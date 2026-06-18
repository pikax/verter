//! `raise_node_to_type_expr` — SemanticNodeId → TypeExpr structural raising
//!
//! Reverse of [`shallow_lower_type_expr`](super::lower::shallow_lower_type_expr).
//! Walks one structural level of a [`SemanticNodeData`] graph payload back
//! into a [`TypeExpr`] tree. No name resolution, no generic substitution,
//! no conditional branch selection — those are the [`PathWalker`](super::walk)'s
//! job. Cycle protection via a per-call `active` visited set.
//!
//! **Authority contract:** this is the *only* `SemanticNodeId →
//! TypeExpr` lowering path in the workspace. Pair with
//! [`shallow_lower_type_expr`](super::lower::shallow_lower_type_expr)
//! (forward direction). The invariant test
//! `semantic_node_to_type_expr_has_exactly_one_path` asserts exactly one
//! `fn raise_node_to_type_expr` exists in `crates/`.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use verter_type_expr::TypeExpr;

use super::ProjectSemanticDispatch;
use crate::instant::Instant;
use crate::resolver_core::component_meta_query_engine::{
    projected_surface_to_type_expr, semantic_query_error_raw, surface_view_to_projected_surface,
    SEMANTIC_OBJECT_SURFACE,
};
use crate::semantic_query::{
    DepSignature, HotTypeRef, IndexKey, MapperKey, OptionalityMod, PathSegment,
    PrimitiveKind as SemanticPrimitiveKind, ProjectionMode, ProjectionReductionContext, QueryError,
    QueryResult, ReadonlyMod, ReductionDemand, ResolveDeclKey, ScopeId, SemanticNodeData,
    SemanticNodeId, SemanticQueryKey, SurfaceMember, SurfaceView, TupleElement,
};

// =====================================================================
// dispatch trace plumbing for cycle-BFS unit tests.
//
// `enable_dispatch_trace_for_test` returns a guard that clears the trace
// on construction and on drop. `execute_read` pushes a static-string
// discriminant for every key it processes. The discriminant index is
// stable (depends only on the variant name), so tests can assert exact
// counts of `Instantiate`, `ResolveDecl`, etc. dispatches.
//
// Plumbing is `#[cfg(test)]`-only: zero footprint outside test builds.
// =====================================================================

#[cfg(test)]
thread_local! {
    pub(crate) static DISPATCH_TRACE: std::cell::RefCell<Vec<&'static str>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
#[allow(dead_code, reason = "consumed by the dispatch-count assertions")]
pub(crate) struct DispatchTraceGuard;

#[cfg(test)]
impl Drop for DispatchTraceGuard {
    fn drop(&mut self) {
        DISPATCH_TRACE.with(|t| t.borrow_mut().clear());
    }
}

#[cfg(test)]
#[allow(dead_code, reason = "consumed by the dispatch-count assertions")]
pub(crate) fn enable_dispatch_trace_for_test() -> DispatchTraceGuard {
    DISPATCH_TRACE.with(|t| t.borrow_mut().clear());
    DispatchTraceGuard
}

// =====================================================================
// Per-key dispatch traffic counter (diagnostic only).
//
// Counts how many times `execute_read` is invoked with each
// `SemanticQueryKey`, keyed by a (variant_discriminant, content_hash)
// digest. Diagnostic-only, `#[cfg(test)]`-gated, zero footprint outside
// test builds.
//
// F18 (r2 review): variant identity uses std::mem::discriminant so the
// digest does NOT need to be updated when adds new
// SemanticQueryKey variants. The discriminant is opaque but stable
// per-variant; pairs with the hash for full key identity.
// =====================================================================

#[cfg(test)]
#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub(crate) struct SemanticQueryKeyDigest {
    pub variant: std::mem::Discriminant<crate::semantic_query::SemanticQueryKey>,
    pub hash: u64,
}

#[cfg(test)]
impl SemanticQueryKeyDigest {
    fn from_key(key: &crate::semantic_query::SemanticQueryKey) -> Self {
        use std::hash::{Hash, Hasher};
        // Supplement §5.D.0 r17 — canonicalise the key BEFORE
        // hashing so probes via the caller's key shape (e.g.
        // `ProjectMember`) hit the same digest the warm cache stores
        // (e.g. `ProjectPath` with a length-1 path). Without this,
        // tests probing `family_cold(&original_key)` always read 0
        // because the warm cache stores the post-canonical form.
        let canonical = canonicalise_for_digest(key);
        let mut hasher = rustc_hash::FxHasher::default();
        canonical.hash(&mut hasher);
        Self {
            variant: std::mem::discriminant(&canonical),
            hash: hasher.finish(),
        }
    }
}

#[cfg(test)]
fn canonicalise_for_digest(
    key: &crate::semantic_query::SemanticQueryKey,
) -> crate::semantic_query::SemanticQueryKey {
    use std::sync::Arc;

    use crate::semantic_query::{PathSegment, SemanticQueryKey};
    match key {
        SemanticQueryKey::ProjectMember { base, member, mode } => SemanticQueryKey::ProjectPath {
            base: *base,
            path: Arc::from(vec![PathSegment::Member(Arc::clone(member))].into_boxed_slice()),
            context: crate::semantic_query::ProjectionReductionContext::published(*mode),
        },
        SemanticQueryKey::IndexedAccess { base, index, mode } => SemanticQueryKey::ProjectPath {
            base: *base,
            path: Arc::from(vec![PathSegment::Index(index.clone())].into_boxed_slice()),
            context: crate::semantic_query::ProjectionReductionContext::published(*mode),
        },
        SemanticQueryKey::NormalizeUnion { members } => SemanticQueryKey::NormalizeUnion {
            members: super::canonicalize_node_list(members),
        },
        SemanticQueryKey::NormalizeIntersection { members } => {
            SemanticQueryKey::NormalizeIntersection {
                members: super::canonicalize_node_list(members),
            }
        }
        other => other.clone(),
    }
}

#[cfg(test)]
thread_local! {
    pub(crate) static DISPATCH_KEY_COUNTS:
        std::cell::RefCell<rustc_hash::FxHashMap<SemanticQueryKeyDigest, u32>> =
        std::cell::RefCell::new(rustc_hash::FxHashMap::default());
    /// Supplement §5.D.0 r17 — per-key COLD dispatch count.
    /// Incremented when `execute_read` enters with no warm cache entry
    /// for the key (cache miss; `build` is invoked).
    pub(crate) static DISPATCH_KEY_COLD_COUNTS:
        std::cell::RefCell<rustc_hash::FxHashMap<SemanticQueryKeyDigest, u32>> =
        std::cell::RefCell::new(rustc_hash::FxHashMap::default());
    /// Supplement §5.D.0 r17 — per-key WARM dispatch count.
    /// Incremented when `execute_read` enters with the key already in
    /// the warm cache (cache hit; `build` is NOT invoked).
    pub(crate) static DISPATCH_KEY_WARM_COUNTS:
        std::cell::RefCell<rustc_hash::FxHashMap<SemanticQueryKeyDigest, u32>> =
        std::cell::RefCell::new(rustc_hash::FxHashMap::default());
}

#[cfg(test)]
pub(crate) fn record_dispatch_key(key: &crate::semantic_query::SemanticQueryKey) {
    let digest = SemanticQueryKeyDigest::from_key(key);
    DISPATCH_KEY_COUNTS.with(|c| {
        *c.borrow_mut().entry(digest).or_insert(0) += 1;
    });
}

/// Supplement §5.D.0 r17 — record a COLD dispatch entry for
/// this key (cache miss, `build` will be invoked).
#[cfg(test)]
pub(crate) fn record_dispatch_cold(key: &crate::semantic_query::SemanticQueryKey) {
    let digest = SemanticQueryKeyDigest::from_key(key);
    DISPATCH_KEY_COLD_COUNTS.with(|c| {
        *c.borrow_mut().entry(digest).or_insert(0) += 1;
    });
}

/// Supplement §5.D.0 r17 — record a WARM dispatch entry for
/// this key (cache hit, returning the memoized value).
#[cfg(test)]
pub(crate) fn record_dispatch_warm(key: &crate::semantic_query::SemanticQueryKey) {
    let digest = SemanticQueryKeyDigest::from_key(key);
    DISPATCH_KEY_WARM_COUNTS.with(|c| {
        *c.borrow_mut().entry(digest).or_insert(0) += 1;
    });
}

/// Supplement §5.D.0 r17 — read the COLD count for `key`.
/// Returns 0 if the key has not been dispatched on this thread since
/// thread start. The counter is monotonic; tests sample baselines and
/// deltas across paired queries.
#[cfg(test)]
pub(crate) fn dispatch_cold_for(key: &crate::semantic_query::SemanticQueryKey) -> usize {
    let digest = SemanticQueryKeyDigest::from_key(key);
    DISPATCH_KEY_COLD_COUNTS.with(|c| c.borrow().get(&digest).copied().unwrap_or(0) as usize)
}

/// Supplement §5.D.0 r17 — read the WARM count for `key`.
#[cfg(test)]
pub(crate) fn dispatch_warm_for(key: &crate::semantic_query::SemanticQueryKey) -> usize {
    let digest = SemanticQueryKeyDigest::from_key(key);
    DISPATCH_KEY_WARM_COUNTS.with(|c| c.borrow().get(&digest).copied().unwrap_or(0) as usize)
}

#[cfg(test)]
fn query_key_discriminant(key: &SemanticQueryKey) -> &'static str {
    match key {
        SemanticQueryKey::ResolveDecl(_) => "ResolveDecl",
        SemanticQueryKey::Instantiate { .. } => "Instantiate",
        SemanticQueryKey::ProjectMember { .. } => "ProjectMember",
        SemanticQueryKey::IndexedAccess { .. } => "IndexedAccess",
        SemanticQueryKey::KeyOf { .. } => "KeyOf",
        SemanticQueryKey::MappedType { .. } => "MappedType",
        SemanticQueryKey::Conditional { .. } => "Conditional",
        SemanticQueryKey::TypeOf { .. } => "TypeOf",
        SemanticQueryKey::NormalizeUnion { .. } => "NormalizeUnion",
        SemanticQueryKey::NormalizeIntersection { .. } => "NormalizeIntersection",
        SemanticQueryKey::ProjectPath { .. } => "ProjectPath",
        SemanticQueryKey::ResolvedNamedType { .. } => "ResolvedNamedType",
        SemanticQueryKey::Relate { .. } => "Relate",
        SemanticQueryKey::ResolveMacroPayload { .. } => "ResolveMacroPayload",
        SemanticQueryKey::ResolveClassSurface { .. } => "ResolveClassSurface",
        SemanticQueryKey::ResolveAmbientNamespace { .. } => "ResolveAmbientNamespace",
        SemanticQueryKey::ResolveEnum { .. } => "ResolveEnum",
        SemanticQueryKey::ResolveOverloadSet { .. } => "ResolveOverloadSet",
        SemanticQueryKey::ApparentType { .. } => "ApparentType",
        SemanticQueryKey::TemplateLiteralReduce { .. } => "TemplateLiteralReduce",
        SemanticQueryKey::FlowNarrowingAt { .. } => "FlowNarrowingAt",
        SemanticQueryKey::ContextualTypeAt { .. } => "ContextualTypeAt",
    }
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// Raise a [`SemanticNodeId`] back to a [`TypeExpr`].
    ///
    /// Pure structural conversion: walks the graph payload one structural
    /// level at a time, raising sub-ids recursively. Cycle protection via
    /// a per-call `active` visited set. Returns `None` when the node is
    /// not present in the host's graph store.
    ///
    /// Use this when you have a `SemanticNodeId` (typically the result of
    /// a [`SemanticQueryApi::execute`] call or a graph-native helper) and
    /// need a [`TypeExpr`] for downstream payload construction. Operator-
    /// shape reduction (`IndexedAccess`, `Conditional`, `Mapped`,
    /// `KeyOf`, `TypeOf`) is the responsibility of the caller — typically
    /// [`Self::raise_and_reduce`]. This function alone is
    /// shell-only.
    pub(crate) fn raise_node_to_type_expr(&self, node: SemanticNodeId) -> Option<TypeExpr> {
        let mut active = FxHashSet::default();
        self.raise_node_to_type_expr_inner(node, &mut active)
    }

    fn raise_node_to_type_expr_inner(
        &self,
        node: SemanticNodeId,
        active: &mut FxHashSet<SemanticNodeId>,
    ) -> Option<TypeExpr> {
        let data = super::node_data_for(self.ctx, node)?;
        Some(match data.as_ref() {
            SemanticNodeData::Primitive(kind) => semantic_primitive_to_type_expr(*kind),
            SemanticNodeData::Literal(value) => TypeExpr::Literal(value.clone()),
            SemanticNodeData::Alias(target) => {
                if !active.insert(node) {
                    return Some(TypeExpr::Unknown {
                        raw: "semanticAliasCycle".to_string(),
                    });
                }
                let result = self.raise_node_to_type_expr_inner(*target, active);
                active.remove(&node);
                return result;
            }
            SemanticNodeData::Union(members) => TypeExpr::Union(std::sync::Arc::from(
                members
                    .iter()
                    .filter_map(|member| self.raise_node_to_type_expr_inner(*member, active))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            )),
            SemanticNodeData::Intersection(members) => {
                // Drop empty-object arms from the Intersection
                // projection. `Id<T> = {} & { [P in keyof T]: T[P] }`
                // and similar helper patterns lower to
                // `Intersection([empty_object, mapped_object])`; the
                // empty arm contributes nothing semantically
                // (`{} & X ≡ X`) but would leak through as a
                // `TypeExpr::Unknown { raw: SEMANTIC_OBJECT_SURFACE }`
                // sentinel which breaks callers that expect a pure
                // Object at the projection boundary. Dropping the
                // semantically-vacuous arm here collapses `{} & X → X`
                // so imported-helper UI bindings materialise cleanly
                // instead of nested in
                // `Intersection([Unknown, Object])`.
                let mut arms: Vec<TypeExpr> = members
                    .iter()
                    .filter_map(|member| self.raise_node_to_type_expr_inner(*member, active))
                    .filter(|arm| !matches!(arm, TypeExpr::Unknown { raw } if raw == SEMANTIC_OBJECT_SURFACE))
                    // `{} & X ≡ X` — the representable empty object (the
                    // Object arm below raises empty surfaces first-class)
                    // is equally vacuous inside an intersection. Known
                    // divergence: for a NULLISH X the pinned tsgo reduces
                    // `{} & null` / `{} & undefined` to `never`, while this
                    // projection-boundary filter collapses them to the
                    // nullish arm. The correct home for that reduction is
                    // the semantic intersection reducer, not this raise
                    // boundary — tracked as debt, predating the first-class
                    // empty-object raise (the retired sentinel filter
                    // collapsed the same way).
                    .filter(|arm| !matches!(arm, TypeExpr::Object(object) if object.properties.is_empty()))
                    .collect();
                if arms.is_empty() {
                    // Every arm was vacuous (`{} & {}`): fall back to the
                    // representable empty object — a zero-arm
                    // `Intersection([])` is not a publishable shape.
                    TypeExpr::Object(std::sync::Arc::new(verter_type_expr::ObjectExpr {
                        properties: Vec::new(),
                    }))
                } else if arms.len() == 1 {
                    arms.pop().unwrap()
                } else {
                    TypeExpr::Intersection(std::sync::Arc::from(arms.into_boxed_slice()))
                }
            }
            SemanticNodeData::Array { element, readonly } => TypeExpr::Array {
                element: std::sync::Arc::new(self.raise_node_to_type_expr_inner(*element, active)?),
                readonly: *readonly,
            },
            SemanticNodeData::Tuple { elements, readonly } => {
                use verter_type_expr::TupleElement;

                TypeExpr::Tuple {
                    elements: std::sync::Arc::from(
                        elements
                            .iter()
                            .filter_map(|element| {
                                Some(TupleElement {
                                    label: element
                                        .label
                                        .as_ref()
                                        .map(|label| label.as_ref().to_string()),
                                    ty: self
                                        .raise_node_to_type_expr_inner(element.value, active)?,
                                    optional: element.optional,
                                    rest: element.rest,
                                })
                            })
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    ),
                    readonly: *readonly,
                }
            }
            SemanticNodeData::Object(surface) => {
                // A genuinely EMPTY surface (no members, no signatures, no
                // index signature) is the representable empty object `{}`
                // — `Pick<T, never>`, `Omit<T, keyof T>`,
                // `NonNullable<unknown>` all reduce to it. Raise it as a
                // zero-property `TypeExpr::Object` instead of the
                // `SEMANTIC_OBJECT_SURFACE` sentinel (the sentinel marks a
                // surface the projection cannot REPRESENT, which an empty
                // object is not). The intersection vacuous-arm rule above
                // drops empty-object arms the same way it drops the
                // sentinel, so `{} & X ≡ X` is preserved.
                if surface.members.is_empty()
                    && surface.call_signatures.is_empty()
                    && surface.construct_signatures.is_empty()
                    && !surface.has_index_signature
                {
                    TypeExpr::Object(std::sync::Arc::new(verter_type_expr::ObjectExpr {
                        properties: Vec::new(),
                    }))
                } else {
                    projected_surface_to_type_expr(&surface_view_to_projected_surface(
                        self.ctx, surface,
                    ))
                    .unwrap_or(TypeExpr::Unknown {
                        raw: SEMANTIC_OBJECT_SURFACE.to_string(),
                    })
                }
            }
            SemanticNodeData::MergedDecl { contributors } => {
                // Peer-merge the same-name interface contributors into one
                // surface (member union + ordered method overload groups) and
                // raise the merged object.
                let merged = self.reduce_merged_decl(contributors);
                return self.raise_node_to_type_expr_inner(merged, active);
            }
            // DeclPlaceholder lowers to a TypeExpr::Ref shell.
            SemanticNodeData::Opaque(QueryError::DeclPlaceholder { name, .. }) => TypeExpr::Ref {
                name: std::sync::Arc::clone(name),
                type_arguments: verter_type_expr::empty_type_args(),
            },
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                ..
            } => TypeExpr::Conditional {
                check: std::sync::Arc::new(self.raise_node_to_type_expr_inner(*check, active)?),
                extends: std::sync::Arc::new(self.raise_node_to_type_expr_inner(*extends, active)?),
                true_type: std::sync::Arc::new(
                    self.raise_node_to_type_expr_inner(*true_branch_ref, active)?,
                ),
                false_type: std::sync::Arc::new(
                    self.raise_node_to_type_expr_inner(*false_branch_ref, active)?,
                ),
            },
            SemanticNodeData::TemplateLiteral {
                quasis,
                expressions,
            } => TypeExpr::TemplateLiteral {
                quasis: quasis
                    .iter()
                    .map(|quasi| quasi.as_ref().to_string())
                    .collect(),
                expressions: std::sync::Arc::from(
                    expressions
                        .iter()
                        .filter_map(|expr| self.raise_node_to_type_expr_inner(*expr, active))
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                ),
            },
            SemanticNodeData::KeyOf { base } => TypeExpr::KeyOf(std::sync::Arc::new(
                self.raise_node_to_type_expr_inner(*base, active)?,
            )),
            SemanticNodeData::IndexedAccess { object, index } => TypeExpr::IndexedAccess {
                object: std::sync::Arc::new(self.raise_node_to_type_expr_inner(*object, active)?),
                index: std::sync::Arc::new(self.index_key_to_type_expr(index, active)?),
            },
            SemanticNodeData::Mapped { mapper, .. } => TypeExpr::Mapped {
                // Presentational projection: look up the binder node
                // by `mapper.parameter_node` and read its
                // `display_name` for the projected
                // `TypeExpr::Mapped { parameter }` field. The
                // semantic-graph interner dedups only
                // structurally-identical binders, so the
                // representative's `display_name` is well-defined.
                parameter: match super::node_data_for(self.ctx, mapper.parameter_node).as_deref() {
                    Some(SemanticNodeData::TypeParam { display_name, .. }) => {
                        display_name.as_ref().to_string()
                    }
                    _ => String::new(),
                },
                source: std::sync::Arc::new(
                    match super::node_data_for(self.ctx, mapper.key_space)?.as_ref() {
                        SemanticNodeData::KeyOf { base } => TypeExpr::KeyOf(std::sync::Arc::new(
                            self.raise_node_to_type_expr_inner(*base, active)?,
                        )),
                        _ => self.raise_node_to_type_expr_inner(mapper.key_space, active)?,
                    },
                ),
                value: std::sync::Arc::new(
                    self.raise_node_to_type_expr_inner(mapper.value_expr, active)?,
                ),
                optional: match mapper.optionality {
                    OptionalityMod::Add => verter_type_expr::MappedModifier::Add,
                    OptionalityMod::Remove => verter_type_expr::MappedModifier::Remove,
                    OptionalityMod::Keep => verter_type_expr::MappedModifier::None,
                },
                readonly: match mapper.readonly {
                    ReadonlyMod::Add => verter_type_expr::MappedModifier::Add,
                    ReadonlyMod::Remove => verter_type_expr::MappedModifier::Remove,
                    ReadonlyMod::Keep => verter_type_expr::MappedModifier::None,
                },
                name_type: match mapper.name_remap {
                    Some(node) => Some(std::sync::Arc::new(
                        self.raise_node_to_type_expr_inner(node, active)?,
                    )),
                    None => None,
                },
            },
            SemanticNodeData::TypeOf(_) => {
                let (value_root, path) = data.typeof_head().expect("TypeOf carrier head");
                let type_args = data.carrier_type_args();
                let mut segments = value_root
                    .name
                    .split('.')
                    .map(|segment| segment.to_string())
                    .collect::<Vec<_>>();
                segments.extend(path.iter().map(|segment| segment.as_ref().to_string()));
                // Raise the instantiation-expression args back onto the
                // projected `ValueRef.type_args` so `typeof C.make<string>`
                // round-trips its arguments. A miss on any arg becomes the
                // `<raise miss>` placeholder so the outer typeof still
                // constructs (mirrors the `ImportType` arm).
                let raised_args: Vec<TypeExpr> = type_args
                    .iter()
                    .map(|id| {
                        self.raise_node_to_type_expr_inner(*id, active).unwrap_or(
                            TypeExpr::Unknown {
                                raw: "<raise miss>".to_string(),
                            },
                        )
                    })
                    .collect();
                TypeExpr::TypeOf(verter_type_expr::ValueRef {
                    path: segments,
                    type_args: raised_args,
                })
            }
            SemanticNodeData::TypeParam {
                display_name,
                constraint,
                default,
                ..
            } => {
                // Project `constraint` / `default` back to `TypeExpr`
                // so the round-trip preserves the declaration shape.
                // The `active` visited set guards against cyclic
                // constraint graphs: when a TypeParam's constraint or
                // default transitively reaches this same node, return
                // `None` from the recursion and drop the field rather
                // than looping.
                //
                // The projected `TypeExpr::TypeParameter.name` uses
                // `display_name` — the human-readable parameter name.
                // `decl` / `param_index` are identity discriminators
                // for structural interning and do not appear in the
                // projected `TypeExpr` shape.
                if !active.insert(node) {
                    return Some(TypeExpr::Unknown {
                        raw: "semanticTypeParamCycle".to_string(),
                    });
                }
                let constraint_expr = constraint
                    .as_ref()
                    .and_then(|c| self.raise_node_to_type_expr_inner(*c, active))
                    .map(std::sync::Arc::new);
                let default_expr = default
                    .as_ref()
                    .and_then(|d| self.raise_node_to_type_expr_inner(*d, active))
                    .map(std::sync::Arc::new);
                active.remove(&node);
                TypeExpr::TypeParameter(verter_type_expr::TypeParam {
                    name: display_name.as_ref().to_string(),
                    constraint: constraint_expr,
                    default: default_expr,
                })
            }
            SemanticNodeData::Infer { name } => TypeExpr::Infer {
                name: name.as_ref().to_string(),
            },
            SemanticNodeData::Opaque(err) => match err {
                QueryError::RecursiveRef { name } => {
                    TypeExpr::recursive_ref(name.as_ref(), Vec::new())
                }
                _ => TypeExpr::Unknown {
                    raw: semantic_query_error_raw(err),
                },
            },
            SemanticNodeData::VueMacroElements(_) => TypeExpr::Unknown {
                raw: "VueMacroElements".to_string(),
            },
            // Canonical Function shape
            // converts back to `TypeExpr::Function`. Session 4 lowered
            // `TypeExpr::Function` → `SemanticNodeData::Function`; this
            // conversion completes the round-trip so alias bodies that
            // include function types (`(() => T)` branches) survive
            // dispatch-only projection without emitting `semanticFunction`
            // sentinels.
            SemanticNodeData::Function {
                params,
                return_type,
                type_parameters,
                signature_span,
                return_type_span,
            } => {
                use verter_type_expr::{FunctionExpr, FunctionParam, FunctionSpans, TypeParam};
                let parameters: Vec<FunctionParam> = params
                    .iter()
                    .filter_map(|p| {
                        Some(FunctionParam::with_span(
                            p.name.as_ref().map(|n| n.as_ref().to_string()),
                            self.raise_node_to_type_expr_inner(p.ty, active)?,
                            p.optional,
                            p.rest,
                            // Carry the graph parameter's span back to the IR.
                            p.span,
                            // The TS-annotation-presence fact is a lowering-time
                            // input to JSDoc `@param` precedence; it is consumed
                            // ONLY by `enrich_params_and_return_with_jsdoc` at
                            // build time, before any graph round-trip. A raised
                            // function is the post-enrichment semantic form and is
                            // never re-enriched, so the graph node does not carry
                            // the fact and `false` is the correct, inert value.
                            false,
                        ))
                    })
                    .collect();
                let return_ty = self
                    .raise_node_to_type_expr_inner(*return_type, active)
                    .map(std::sync::Arc::new);
                let type_params: Vec<TypeParam> = type_parameters
                    .iter()
                    .map(|tp| TypeParam {
                        name: tp.name.as_ref().to_string(),
                        constraint: tp
                            .constraint
                            .and_then(|c| self.raise_node_to_type_expr_inner(c, active))
                            .map(std::sync::Arc::new),
                        default: tp
                            .default
                            .and_then(|d| self.raise_node_to_type_expr_inner(d, active))
                            .map(std::sync::Arc::new),
                    })
                    .collect();
                // Carry the graph Function node's signature / return spans back
                // to the IR (round-trip provenance preservation).
                TypeExpr::Function(std::sync::Arc::new(FunctionExpr::with_spans(
                    parameters,
                    return_ty,
                    type_params,
                    FunctionSpans {
                        signature: *signature_span,
                        return_type: *return_type_span,
                    },
                )))
            }
            // Lazy carriers. DeclRef raises to a
            // bare `Ref { name }` with empty type arguments. Identity
            // (`canonical_id + whole_hash`) is encoded in the interning
            // scope, not in the projected TypeExpr — that's the lossy
            // direction of the Navigate-mode lazy lowering.
            SemanticNodeData::DeclRef { identity } => TypeExpr::Ref {
                name: std::sync::Arc::clone(&identity.decl_name),
                type_arguments: verter_type_expr::empty_type_args(),
            },
            // InstantiationRef raises to `Ref { name, type_arguments: [...] }`.
            // A miss on any arg raise becomes `Unknown { raw: "<raise
            // miss>" }` so the outer Ref still constructs (vs. failing
            // the whole raise which would lose the application shape).
            SemanticNodeData::InstantiationRef { base, args } => {
                let raised_args: Vec<TypeExpr> = args
                    .iter()
                    .map(|id| {
                        self.raise_node_to_type_expr_inner(*id, active).unwrap_or(
                            TypeExpr::Unknown {
                                raw: "<raise miss>".to_string(),
                            },
                        )
                    })
                    .collect();
                TypeExpr::Ref {
                    name: std::sync::Arc::clone(&base.decl_name),
                    type_arguments: std::sync::Arc::from(raised_args.into_boxed_slice()),
                }
            }
            // Unresolved bare-name carrier → `Ref { name, type_arguments }`
            // (the shallow-by-default published shape). `scope` is value-side
            // resolution context, not part of the projected shape. The
            // structurally-lowered `type_args` raise back onto
            // `Ref.type_arguments` so `Foo<Arg>` round-trips its arguments;
            // an empty slice raises to the bare `Foo` case. A miss on any
            // arg becomes the `<raise miss>` placeholder so the outer
            // reference still constructs (mirrors the `ImportType` arm).
            SemanticNodeData::BareRef(_) => {
                let (name, _scope) = data.bare_ref_head().expect("BareRef carrier head");
                let type_args = data.carrier_type_args();
                let raised_args: Vec<TypeExpr> = type_args
                    .iter()
                    .map(|id| {
                        self.raise_node_to_type_expr_inner(*id, active).unwrap_or(
                            TypeExpr::Unknown {
                                raw: "<raise miss>".to_string(),
                            },
                        )
                    })
                    .collect();
                TypeExpr::Ref {
                    name: std::sync::Arc::clone(name),
                    type_arguments: std::sync::Arc::from(raised_args.into_boxed_slice()),
                }
            }
            // Unresolved dynamic-import carrier → `TypeExpr::ImportType`.
            // A miss on any type-arg raise becomes the `<raise miss>`
            // placeholder so the outer import-type still constructs.
            SemanticNodeData::ImportType(_) => {
                let (specifier, qualifier, typeof_query) =
                    data.import_type_head().expect("ImportType carrier head");
                let type_args = data.carrier_type_args();
                let raised_args: Vec<TypeExpr> = type_args
                    .iter()
                    .map(|id| {
                        self.raise_node_to_type_expr_inner(*id, active).unwrap_or(
                            TypeExpr::Unknown {
                                raw: "<raise miss>".to_string(),
                            },
                        )
                    })
                    .collect();
                TypeExpr::ImportType {
                    specifier: std::sync::Arc::clone(specifier),
                    qualifier: std::sync::Arc::clone(qualifier),
                    typeof_query,
                    type_arguments: std::sync::Arc::from(raised_args.into_boxed_slice()),
                }
            }
            // Raw-fallback carrier → `TypeExpr::Unknown { raw }` (the
            // round-trip of the only carrier that holds raw type text).
            SemanticNodeData::RawFallback { raw } => TypeExpr::Unknown {
                raw: raw.as_ref().to_string(),
            },
            // Constructor-type carrier → `TypeExpr::ConstructorType`. The
            // signature interns a `Function` node; rewrap its raised
            // `FunctionExpr` as a constructor type. Any non-function shape
            // (malformed carrier) is preserved as raised rather than
            // fabricated.
            SemanticNodeData::ConstructorType { signature } => {
                let raised = self.raise_node_to_type_expr_inner(*signature, active)?;
                if let TypeExpr::Function(func) = &raised {
                    TypeExpr::ConstructorType(std::sync::Arc::clone(func))
                } else {
                    raised
                }
            }
            // Synthetic-binding carrier → `TypeExpr::SyntheticSlotBinding`.
            // Re-hydrate the full carrier key by re-attaching the value-side
            // `value_node` provenance ordinal to the content-free identity.
            SemanticNodeData::SyntheticBinding { id, value_node } => {
                TypeExpr::SyntheticSlotBinding(std::sync::Arc::new(id.to_carrier_key(*value_node)))
            }
        })
    }

    /// The single reverse / materialisation boundary: project a
    /// [`HotTypeRef`] handle back to a compat [`TypeExpr`].
    ///
    /// This is the output/compat seam — it is the named boundary that the
    /// migration collapses all `TypeExpr` materialisation onto. It is a
    /// TOTAL wrapper over the shell-only [`Self::raise_node_to_type_expr`]
    /// primitive: every carrier round-trips here (raw-fallback text →
    /// `Unknown { raw }`, the synthetic binding, the constructor carrier, and
    /// the `RecursiveRef` back-edge via `Opaque(QueryError::RecursiveRef)`).
    /// Tuple-element rest fidelity round-trips separately through the `Tuple`
    /// arm's `TupleElement.rest`; there is no standalone `Rest` carrier. A
    /// deref miss (a stale handle whose node is out of the live arena)
    /// yields a `"<materialize miss>"` fallback rather than panicking.
    ///
    /// Operator-shape REDUCTION (`IndexedAccess` / `Conditional` / `Mapped`
    /// / `KeyOf` / `TypeOf`) is NOT performed here — that is
    /// [`Self::raise_and_reduce`]'s job; this boundary is shell-only, like
    /// the primitive it wraps.
    // The production callers (output adapters / diagnostics / compat
    // exporters) collapse onto this boundary as the hot path migrates off
    // stored `TypeExpr` bodies; it is exercised today by the carrier
    // round-trip unit tests.
    #[allow(dead_code)]
    #[must_use]
    pub(crate) fn materialize_type_expr(&self, handle: HotTypeRef) -> TypeExpr {
        self.raise_node_to_type_expr(handle.node())
            .unwrap_or(TypeExpr::Unknown {
                raw: "<materialize miss>".to_string(),
            })
    }

    fn index_key_to_type_expr(
        &self,
        index: &IndexKey,
        active: &mut FxHashSet<SemanticNodeId>,
    ) -> Option<TypeExpr> {
        Some(match index {
            IndexKey::String(text) => TypeExpr::string_literal(text.as_ref()),
            IndexKey::Number(number) => TypeExpr::number_literal(number.get() as f64),
            IndexKey::TypeNode(node) => self.raise_node_to_type_expr_inner(*node, active)?,
        })
    }

    /// `execute` variant that returns the full [`CacheRead`].
    ///
    /// `ProjectSemanticDispatch::execute` (the [`SemanticQueryApi`] trait
    /// method) discards the dep-signature half of the cache read; this
    /// variant keeps it so callers like [`Self::raise_and_reduce`] can
    /// accumulate dep facts across nested dispatches and merge them into
    /// the session-layer `fact_versions`. This is the dispatch entry the
    /// cold-build subtree reducer and the operator sub-reductions
    /// (`ProjectPath` / `NormalizeIntersection` / macro-payload
    /// intersection normalisation) ride so their dependency facts are not
    /// dropped — the dep-signature-preserving peer of the `SemanticQueryApi`
    /// trait's `execute`.
    pub(crate) fn execute_read(
        &self,
        key: SemanticQueryKey,
    ) -> crate::semantic_query::CacheRead<QueryResult<SemanticNodeId>> {
        // Trace the variant for cycle-BFS unit tests.
        // Records the variant before key canonicalisation so the
        // observed call shape matches the caller's intent (sugar
        // variants are recorded as the caller wrote them).
        #[cfg(test)]
        DISPATCH_TRACE.with(|t| t.borrow_mut().push(query_key_discriminant(&key)));

        // Per-key dispatch traffic counter. Records a
        // (variant_discriminant, content_hash) digest so diagnostic
        // tests can dump the top-N most-dispatched keys.
        #[cfg(test)]
        record_dispatch_key(&key);

        // Delegate to the shared cold-build helper. The helper handles
        // canonicalisation, sentinel construction, the tracer-wrapped
        // build closure, and the warm-hit fast path inside
        // `execute_cooperative`. Routing both `execute` and
        // `execute_read` through one helper ensures fact-tracer
        // installation never bypasses any cold-build path.
        self.execute_via_cold_build_helper(key)
    }

    /// Reduce a [`SemanticNodeId`] by dispatching the appropriate
    /// [`SemanticQueryKey`] for each operator-shape encountered in the
    /// graph subtree, then raise the fully-reduced graph node to a
    /// [`TypeExpr`].
    ///
    /// Operates GRAPH-NATIVE: walks [`SemanticNodeData`] via the
    /// graph's `node_data`, dispatches per shape, interns reduced
    /// shells via [`crate::semantic_query_memo::SemanticGraphStore::intern_preserving_scope`].
    /// No re-lowering of raised TypeExpr subtrees.
    ///
    /// `mode` is threaded into nested dispatches and determines whether
    /// `DeclRef` / `InstantiationRef` reduce eagerly (Expanded) or stay
    /// terminal (Navigate).
    ///
    /// Returns a [`MaterializedTypeExpr`] carrying the producing
    /// `SemanticNodeId`, the raised `TypeExpr`, and the accumulated
    /// `DepSignature`.
    ///
    /// Backwards-compatible entry — defaults to a
    /// `Published(mode)` reduction context. Callers that need the
    /// reduction-demand axis (`Published` vs `StructuralTransit`) go
    /// through [`Self::raise_and_reduce_with_context`].
    // Published(mode)-default convenience wrapper over
    // `raise_and_reduce_with_context`; exercised by the dispatch reducer tests.
    #[allow(dead_code)]
    pub(crate) fn raise_and_reduce(
        &self,
        node: SemanticNodeId,
        mode: ProjectionMode,
    ) -> MaterializedTypeExpr {
        self.raise_and_reduce_with_context(node, ProjectionReductionContext::published(mode))
    }

    /// Context-explicit variant of [`Self::raise_and_reduce`]
    /// (demand-driven reducer spec).
    ///
    /// The caller supplies the publication
    /// [`ProjectionReductionContext`] that flows into every operator
    /// dispatch and propagates through child traversal selection.
    /// `Published(Expanded)` keeps whole-surface behaviour;
    /// `Published(Navigate)` is the per-prop publication boundary that
    /// stops at the demanded terminal without breadth-enumerating
    /// composite members or inactive conditional branches.
    #[allow(dead_code)] // wired by projector per-member entry.
    pub(crate) fn raise_and_reduce_with_context(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
    ) -> MaterializedTypeExpr {
        let mut state = ReduceState::default();
        let reduced = self.reduce_graph_node_iterative(node, context, &mut state);
        let type_expr = self
            .raise_node_to_type_expr(reduced)
            .unwrap_or(TypeExpr::Unknown {
                raw: "<raise miss after reduction>".to_string(),
            });
        let result_is_partial = state.result_is_partial;
        MaterializedTypeExpr {
            node_id: Some(reduced),
            type_expr,
            dep_signature: state.into_dep_signature(),
            result_is_partial,
        }
    }

    /// Top-down demand-driven graph reducer
    /// (demand-driven reducer spec — stack-safe).
    ///
    /// Replaces the legacy bottom-up topological reducer. The
    /// pre-walk visited the ENTIRE reachable subgraph and then reduced
    /// every visited node — that meant `keyof T` / `{ [K in S]: V }`
    /// inside non-selected conditional branches, generic arguments, or
    /// mapped value bodies dispatched their own `KeyOf` /
    /// `MappedType` keys under `Published(Expanded)`, reifying
    /// `outputSchema` / `execute` per-member edges that no caller asked
    /// for. The demand-driven design treats `Published` as "this exact
    /// node is the demanded terminal of the current step": composite
    /// children, inactive branches, and operator-operand subgraphs are
    /// only descended into when the publication boundary demands them.
    ///
    /// Iterative work-stack with two frame kinds:
    ///
    /// - `Descend(node, context)` — entry frame. Marks
    ///   `(node, context)` visited and pushes a `Reduce(node, context)`
    ///   frame followed by the children selected for that node + context
    ///   (see [`Self::push_demand_children`]). Children are pushed AFTER
    ///   the Reduce frame so they pop FIRST — the LIFO stack acts as a
    ///   post-order traversal.
    /// - `Reduce(node, context)` — closure frame. All selected children
    ///   have been reduced into `state.mapping`; invoke
    ///   `reduce_one(node, context, state)` to produce this node's
    ///   reduction.
    ///
    /// `visited` and `mapping` are keyed by
    /// `(SemanticNodeId, ProjectionReductionContext)` so a
    /// `StructuralTransit` reduction does not collide with a
    /// `Published` reduction on the same node — they are distinct
    /// evaluations.
    ///
    /// Stack-safe for arbitrarily deep acyclic structures (≥5000
    /// levels; verified by stack-safety regression fixtures in §4.1).
    #[allow(dead_code)] // wired by raise_and_reduce above.
    fn reduce_graph_node_iterative(
        &self,
        root: SemanticNodeId,
        root_context: ProjectionReductionContext,
        state: &mut ReduceState,
    ) -> SemanticNodeId {
        // Loop-5 instrumentation — count every iterative-reduction
        // entry. One `raise_and_reduce` produces exactly one of these.
        crate::loop5_instrumentation::RAISE_REDUCE_GRAPH_NODE_ITERATIVE_CALLS
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Loop-8 instrumentation — wall-clock attribution for the
        // iterative-reduction body. Calls counter already bumped
        // above (Loop 5); this guard adds NS-only on drop.
        let _loop8_timer = crate::loop5_instrumentation::TimerGuard::new_ns_only(
            &crate::loop5_instrumentation::REDUCE_GRAPH_NODE_ITERATIVE_NS,
        );

        self.reduce_subtree(root, root_context, state)
    }

    /// Reduce the subtree rooted at `root` under `context` using the
    /// top-down demand-driven work stack. Shares `state.visited` /
    /// `state.mapping` with the caller so cycles are broken once and a
    /// dispatch-produced new node can re-enter the reducer without
    /// re-traversing already-resolved subgraphs.
    ///
    /// Each `(node, context)` pair reduces at most once per call to
    /// [`Self::raise_and_reduce_with_context`] — the visited set
    /// deduplicates entry.
    #[allow(dead_code)] // wired by reduce_graph_node_iterative + dispatch_operator_with_recurse.
    fn reduce_subtree(
        &self,
        root: SemanticNodeId,
        root_context: ProjectionReductionContext,
        state: &mut ReduceState,
    ) -> SemanticNodeId {
        if let Some(&already) = state.mapping.get(&(root, root_context)) {
            return already;
        }
        let mut stack: Vec<ReduceFrame> = Vec::with_capacity(8);
        stack.push(ReduceFrame {
            node: root,
            context: root_context,
            kind: ReduceFrameKind::Descend,
        });
        while let Some(frame) = stack.pop() {
            match frame.kind {
                ReduceFrameKind::Descend => {
                    if !state.visited.insert((frame.node, frame.context)) {
                        // Already in-progress or completed under this
                        // context. Mapping carries the reduction once
                        // it lands; cyclic re-entry returns the raw
                        // node via the rebuild_* fall-through.
                        continue;
                    }
                    let Some(data) = super::node_data_for(self.ctx, frame.node) else {
                        // No graph data — record a self-identity
                        // reduction so callers reading `mapping`
                        // get the raw node.
                        state
                            .mapping
                            .insert((frame.node, frame.context), frame.node);
                        continue;
                    };
                    // Reduce frame is pushed FIRST so it pops AFTER
                    // children (LIFO post-order).
                    stack.push(ReduceFrame {
                        node: frame.node,
                        context: frame.context,
                        kind: ReduceFrameKind::Reduce,
                    });
                    self.push_demand_children(&data, frame.context, &mut stack);
                }
                ReduceFrameKind::Reduce => {
                    let reduced = self.reduce_one(frame.node, frame.context, state);
                    state.mapping.insert((frame.node, frame.context), reduced);
                }
            }
        }
        state
            .mapping
            .get(&(root, root_context))
            .copied()
            .unwrap_or(root)
    }

    /// Push child frames for `data` onto `stack` according to the
    /// demand-driven traversal rules.
    ///
    /// The rules are:
    ///
    /// - Aliases (`Alias`) push their target with the SAME context —
    ///   aliases are semantically transparent and inherit the caller's
    ///   publication demand.
    /// - Operator operands (`KeyOf.base`, `Mapped.source`,
    ///   `Conditional.check`/`extends`, `IndexedAccess.index` typed
    ///   nodes) push as `StructuralTransit`. Their nested operators
    ///   carrier-stop under [`super::may_reduce_operator`], so they
    ///   contribute their structural shape without reifying their own
    ///   members.
    /// - `IndexedAccess.object` pushes as `Published(Navigate)` under
    ///   any `Published` parent (`Foo['a']['b']` walks the path
    ///   navigate-only at intermediate hops; the terminal hop carries
    ///   the caller's mode through the dispatch). Under
    ///   `StructuralTransit` parents, the object pushes as
    ///   `StructuralTransit` (no path materialisation needed for a
    ///   transit walk).
    /// - `Conditional` branches are NOT pushed. The conditional
    ///   dispatch returns the selected branch as its result; that
    ///   branch is then reduced inline via
    ///   [`Self::dispatch_operator_with_recurse`]. Inactive branches
    ///   are never visited — this is the leak fix.
    /// - `Mapped.value_expr` / `Mapped.name_remap` / `Mapped.parameter_node`
    ///   are NOT pushed. The `MappedType` dispatch substitutes the
    ///   binder and evaluates per-key internally under
    ///   `StructuralTransit` (see `build.rs:1817` / `1852`).
    /// - `TypeOf` carries `value_root` + path segments — no semantic
    ///   children to descend.
    /// - Composite shapes (`Object` members, `Union` /
    ///   `Intersection` arms, `Tuple` elements, `Array` element,
    ///   `Function` params / return / type-param constraints/defaults)
    ///   are pushed ONLY under whole-surface `Published(Expanded)`.
    ///   Per the spec, per-prop `Published(Navigate)` /
    ///   `Published(Shallow)` and `StructuralTransit` callers do NOT
    ///   traverse composite children — the parent IS the demand
    ///   terminal. This is the structural leak fix: a per-member
    ///   publication that resolves to an Object stays shallow at the
    ///   object surface.
    /// - `InstantiationRef.args` push under the same context the
    ///   carrier reduces under (args become substituted into the
    ///   instantiated body; their demand follows the body's demand).
    /// - Terminals (`Primitive`, `Literal`, `TypeParam`, `Opaque`,
    ///   `Infer`, `TemplateLiteral`, `VueMacroElements`, `DeclRef`)
    ///   have no semantic operand children for the iterative reducer
    ///   to pre-resolve.
    #[allow(dead_code)] // wired by reduce_subtree above.
    fn push_demand_children(
        &self,
        data: &SemanticNodeData,
        parent_context: ProjectionReductionContext,
        stack: &mut Vec<ReduceFrame>,
    ) {
        match data {
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::Opaque(_)
            | SemanticNodeData::Infer { .. }
            | SemanticNodeData::TemplateLiteral { .. }
            | SemanticNodeData::TypeOf(_)
            | SemanticNodeData::VueMacroElements(_)
            | SemanticNodeData::DeclRef { .. }
            // Unresolved bare-name / dynamic-import / raw-fallback /
            // synthetic-binding carriers are resolved as a whole by the
            // dispatch, not rebuilt by this reducer, so they expose no
            // operand children to pre-resolve here.
            | SemanticNodeData::BareRef(_)
            | SemanticNodeData::ImportType(_)
            | SemanticNodeData::RawFallback { .. }
            | SemanticNodeData::SyntheticBinding { .. } => {}
            SemanticNodeData::Alias(target) => {
                stack.push(ReduceFrame::descend(*target, parent_context));
            }
            // Composite shapes — push children ONLY under whole-surface
            // `Published(Expanded)`. Per-prop / structural-transit
            // parents skip composite descent (the parent is the demand
            // terminal).
            SemanticNodeData::Object(view) => {
                if is_whole_surface_published(parent_context) {
                    for member in view.members.iter() {
                        stack.push(ReduceFrame::descend(member.value, parent_context));
                    }
                    for sig in view.call_signatures.iter() {
                        stack.push(ReduceFrame::descend(*sig, parent_context));
                    }
                    for sig in view.construct_signatures.iter() {
                        stack.push(ReduceFrame::descend(*sig, parent_context));
                    }
                    for sig in view.index_signatures.iter() {
                        stack.push(ReduceFrame::descend(sig.key_type, parent_context));
                        stack.push(ReduceFrame::descend(sig.value_type, parent_context));
                    }
                    if let Some(ks) = view.keyspace {
                        stack.push(ReduceFrame::descend(ks, parent_context));
                    }
                }
            }
            SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
                if is_whole_surface_published(parent_context) {
                    for arm in arms.iter() {
                        stack.push(ReduceFrame::descend(*arm, parent_context));
                    }
                }
            }
            SemanticNodeData::Array { element, .. } => {
                if is_whole_surface_published(parent_context) {
                    stack.push(ReduceFrame::descend(*element, parent_context));
                }
            }
            SemanticNodeData::Tuple { elements, .. } => {
                if is_whole_surface_published(parent_context) {
                    for el in elements.iter() {
                        stack.push(ReduceFrame::descend(el.value, parent_context));
                    }
                }
            }
            SemanticNodeData::Function {
                params,
                return_type,
                type_parameters,
                ..
            } => {
                if is_whole_surface_published(parent_context) {
                    for p in params.iter() {
                        stack.push(ReduceFrame::descend(p.ty, parent_context));
                    }
                    stack.push(ReduceFrame::descend(*return_type, parent_context));
                    for tp in type_parameters.iter() {
                        if let Some(c) = tp.constraint {
                            stack.push(ReduceFrame::descend(c, parent_context));
                        }
                        if let Some(d) = tp.default {
                            stack.push(ReduceFrame::descend(d, parent_context));
                        }
                    }
                }
            }
            // Operator shapes — operands push as StructuralTransit
            // (or Published(Navigate) for IndexedAccess.object under a
            // Published parent). Branches / mapper-internal nodes are
            // NEVER eagerly pushed — the dispatch picks the selected
            // branch or evaluates the mapped value per-key.
            SemanticNodeData::KeyOf { base } => {
                stack.push(ReduceFrame::descend(
                    *base,
                    ProjectionReductionContext::structural_transit(),
                ));
            }
            SemanticNodeData::IndexedAccess { object, index } => {
                let object_context = indexed_access_object_context(parent_context);
                stack.push(ReduceFrame::descend(*object, object_context));
                if let IndexKey::TypeNode(n) = index {
                    stack.push(ReduceFrame::descend(
                        *n,
                        ProjectionReductionContext::structural_transit(),
                    ));
                }
            }
            SemanticNodeData::Mapped { source, .. } => {
                // Source pushes as StructuralTransit for key enumeration.
                // value_expr / name_remap / parameter_node DO NOT push —
                // the MappedType dispatch substitutes them per-key under
                // an internal StructuralTransit evaluation (see
                // build.rs:1817 / 1852).
                stack.push(ReduceFrame::descend(
                    *source,
                    ProjectionReductionContext::structural_transit(),
                ));
            }
            SemanticNodeData::Conditional { check, extends, .. } => {
                // Check / extends reduce structurally so the conditional
                // dispatch can decide the selected branch. The branches
                // themselves are NOT pre-pushed — demand-driven: only the
                // SELECTED branch is reduced (via the dispatch result).
                stack.push(ReduceFrame::descend(
                    *check,
                    ProjectionReductionContext::structural_transit(),
                ));
                stack.push(ReduceFrame::descend(
                    *extends,
                    ProjectionReductionContext::structural_transit(),
                ));
            }
            SemanticNodeData::TypeParam {
                constraint,
                default,
                ..
            } => {
                if is_whole_surface_published(parent_context) {
                    if let Some(c) = constraint {
                        stack.push(ReduceFrame::descend(*c, parent_context));
                    }
                    if let Some(d) = default {
                        stack.push(ReduceFrame::descend(*d, parent_context));
                    }
                }
            }
            SemanticNodeData::InstantiationRef { args, .. } => {
                // Args travel with the carrier — substituted into the
                // body if the carrier's dispatch reifies. Under
                // Navigate the carrier stays terminal so args effectively
                // stay un-reduced via the mapping fall-through.
                for arg in args.iter() {
                    stack.push(ReduceFrame::descend(*arg, parent_context));
                }
            }
            SemanticNodeData::MergedDecl { contributors } => {
                // Same-name merged contributors descend like intersection arms
                // under whole-surface publication.
                if is_whole_surface_published(parent_context) {
                    for contributor in contributors.iter() {
                        stack.push(ReduceFrame::descend(*contributor, parent_context));
                    }
                }
            }
            // The constructor signature descends like the function
            // signature under whole-surface publication.
            SemanticNodeData::ConstructorType { signature } => {
                if is_whole_surface_published(parent_context) {
                    stack.push(ReduceFrame::descend(*signature, parent_context));
                }
            }
        }
    }

    /// Reduce a single node assuming all its demand-selected children
    /// have already been reduced into `state.mapping`. Returns the
    /// reduced `SemanticNodeId`.
    ///
    /// `context` carries the publication / structural-transit demand;
    /// child lookups in `state.mapping` are keyed by
    /// `(child_node, child_context)` where `child_context` is derived
    /// per the demand-driven traversal rules (see
    /// [`Self::push_demand_children`]).
    ///
    /// Per-shape table:
    /// - Operator shapes (`IndexedAccess`, `KeyOf`, `Conditional`,
    ///   `Mapped`, `TypeOf`) dispatch the matching `SemanticQueryKey`
    ///   with the caller's `context`. `Value(reduced)` with `reduced
    ///   != node` recurses through [`Self::dispatch_operator_with_recurse`];
    ///   `Value(node)` (deferred over a free type parameter or
    ///   carrier-stop) accepts the form.
    /// - `DeclRef` / `InstantiationRef`: in `Published(Navigate)` /
    ///   `StructuralTransit`, terminal (DeclRef still follows aliases
    ///   because aliases are semantically transparent). In
    ///   `Published(Expanded)`, dispatch `ResolveDecl` /
    ///   `Instantiate`.
    /// - Composite shapes (`Object` / `Union` / `Intersection` /
    ///   `Array` / `Tuple` / `Function`) rebuild via
    ///   `intern_preserving_scope` when any child reduced; else return
    ///   `node` unchanged. Child reductions only land in `mapping`
    ///   under whole-surface `Published(Expanded)` (demand rule)
    ///   — so non-whole-surface contexts return the parent verbatim.
    /// - `TemplateLiteral` / `Infer` hard-stops have no dispatch
    ///   variant and become `Unknown { raw: "<…>" }`.
    /// - Terminals (`Primitive` / `Literal` / `TypeParam` / `Opaque(…)`)
    ///   return `node` as-is.
    #[allow(dead_code)] // wired by reduce_graph_node_iterative above.
    fn reduce_one(
        &self,
        node: SemanticNodeId,
        context: ProjectionReductionContext,
        state: &mut ReduceState,
    ) -> SemanticNodeId {
        let Some(data) = super::node_data_for(self.ctx, node) else {
            return node;
        };
        let mode = context.mode;
        match data.as_ref() {
            // --- DeclPlaceholder unwrap (cross-file decl carrier) ---
            //
            // `ResolveDecl` returns
            // `Opaque(DeclPlaceholder { canonical_id, name, whole_hash })`
            // as a deferred carrier for cross-file declarations.
            //
            // The demand reducer unwraps the placeholder via the
            // matching `Instantiate { args: [] }` ONLY when the caller's
            // demand resolves deeply through cross-file aliases:
            //
            // - `Published(Expanded)` whole-surface — whole-surface
            //   materialisation deep-resolves cross-file aliases.
            // - `Published(Navigate)` — the navigate publication
            //   inherits the consumer's path-precision intent:
            //   intermediate hops (`IndexedAccess.object`,
            //   `DeclRef` followed through an IA chain) resolve to
            //   their bodies so the next hop / index lookup can
            //   pattern-match. The body is materialised but the
            //   surface stays shallow under Navigate because
            //   `push_demand_children` does not push composite
            //   children.
            //
            // Under `StructuralTransit` the carrier stays in place —
            // transit walks observe the placeholder structurally.
            //
            // The architectural "package-backed alias stays shallow"
            // rule is enforced at the PROJECTOR layer
            // (`type_expr_has_package_backed_object_like_root_with_fence`
            // gate) — not inside the reducer. Reducer-time unwrap
            // is unconditional within a `Published` demand because
            // the projector has already decided the alias chain is
            // workspace-resolvable.
            SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                canonical_id,
                name,
                whole_hash,
            }) => {
                if matches!(context.demand, ReductionDemand::StructuralTransit) {
                    return node;
                }
                // Architectural rule: package-backed alias names stay
                // shallow at the publication boundary. The placeholder
                // for a `node_modules`-resident declaration is the
                // intentional carrier — unwrapping it would inline the
                // package alias's body into the published surface and
                // violate the "imported alias names (workspace-owned
                // OR package-backed) — stay shallow regardless of
                // where they live" half of the shallow-by-default
                // rule. The projector-layer gate
                // (`type_expr_has_package_backed_object_like_root_with_fence`)
                // only sees the OUTER raised root; a workspace-rooted
                // IA whose value lands on a package-backed alias
                // bypasses it. This check is the reducer-side mirror.
                if self.ctx.workspace_is_package_backed(canonical_id.as_ref()) {
                    return node;
                }
                // The placeholder's `whole_hash` is diagnostic payload only;
                // the `Instantiate` key is content-free (R6) and the cold
                // build re-sources the live whole_hash from
                // `ensure_indexed_ready_serve`.
                let _ = whole_hash;
                let base = self.type_slot_for(Arc::clone(canonical_id), Arc::clone(name));
                let key = SemanticQueryKey::Instantiate {
                    context: self.instantiate_context_for(&base.defining_canonical, context),
                    base,
                    args: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                };
                self.dispatch_operator_with_recurse(node, key, context, state)
            }

            // --- terminal shapes ---
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::TypeParam { .. }
            | SemanticNodeData::Opaque(_)
            | SemanticNodeData::VueMacroElements(_)
            // Unresolved bare-name / dynamic-import / raw-fallback /
            // synthetic-binding carriers pass through this reducer
            // unchanged — the dispatch resolves them as a whole.
            | SemanticNodeData::BareRef(_)
            | SemanticNodeData::ImportType(_)
            | SemanticNodeData::RawFallback { .. }
            | SemanticNodeData::SyntheticBinding { .. } => node,

            // --- hard-stop operator shapes (no dispatch variant) ---
            SemanticNodeData::TemplateLiteral { .. } => {
                self.opaque_unknown_with(node, "<unresolved template literal type>")
            }
            SemanticNodeData::Infer { .. } => {
                self.opaque_unknown_with(node, "<unresolved infer type>")
            }

            // --- alias unwrap: follow target's reduction ---
            SemanticNodeData::Alias(target) => state
                .mapping
                .get(&(*target, context))
                .copied()
                .unwrap_or(*target),

            // --- operator dispatches (context-aware via underlying key) ---
            SemanticNodeData::IndexedAccess { object, index } => {
                let object_context = indexed_access_object_context(context);
                let object = state
                    .mapping
                    .get(&(*object, object_context))
                    .copied()
                    .unwrap_or(*object);
                let index = index.clone();
                self.dispatch_operator_with_recurse(
                    node,
                    SemanticQueryKey::IndexedAccess {
                        base: object,
                        index,
                        mode,
                    },
                    context,
                    state,
                )
            }
            SemanticNodeData::KeyOf { base } => {
                let base_context = ProjectionReductionContext::structural_transit();
                let base = state
                    .mapping
                    .get(&(*base, base_context))
                    .copied()
                    .unwrap_or(*base);
                // raise.rs is a publication-path consumer (the bounded
                // fixed-point reducer + the typed-IR raise). Forward
                // the caller's context — `Published(Expanded)` reifies
                // the keyspace; `Published(Navigate)` /
                // `StructuralTransit` carrier-stop per
                // `may_reduce_operator`.
                self.dispatch_operator_with_recurse(
                    node,
                    SemanticQueryKey::KeyOf { base, context },
                    context,
                    state,
                )
            }
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                distributive,
            } => {
                let operand_context = ProjectionReductionContext::structural_transit();
                let check = state
                    .mapping
                    .get(&(*check, operand_context))
                    .copied()
                    .unwrap_or(*check);
                let extends = state
                    .mapping
                    .get(&(*extends, operand_context))
                    .copied()
                    .unwrap_or(*extends);
                // Branches are NOT pre-pushed — the dispatch picks the
                // selected branch and `dispatch_operator_with_recurse`
                // reduces only that branch under the caller's context
                // (demand-driven: "reduce only the selected branch").
                let true_branch = *true_branch_ref;
                let false_branch = *false_branch_ref;
                let distributive = *distributive;
                self.dispatch_operator_with_recurse(
                    node,
                    SemanticQueryKey::Conditional {
                        check,
                        extends,
                        true_branch,
                        false_branch,
                        distributive,
                    },
                    context,
                    state,
                )
            }
            SemanticNodeData::Mapped { source, mapper } => {
                let source_context = ProjectionReductionContext::structural_transit();
                let source = state
                    .mapping
                    .get(&(*source, source_context))
                    .copied()
                    .unwrap_or(*source);
                // Re-key the mapper's `key_space` from any reduction the
                // source push produced. `value_expr` / `name_remap` /
                // `parameter_node` are NOT in mapping — the dispatch
                // substitutes the binder and evaluates per-key
                // internally.
                let mapper = remap_mapper(mapper, &state.mapping, source_context);
                self.dispatch_operator_with_recurse(
                    node,
                    SemanticQueryKey::MappedType {
                        source,
                        mapper,
                        context,
                    },
                    context,
                    state,
                )
            }
            SemanticNodeData::TypeOf(_) => {
                // `typeof value.path<args>`: resolve the value root through the
                // single typeof query, PROJECT the carrier's dotted path, THEN
                // apply the carrier's instantiation `type_args` to the projected
                // signature (resolve → project → apply, mirroring the eager
                // lowering order and the evaluate/walk arms). This is the
                // SEMANTIC reduction path (the structural raise/round-trip
                // preserves `type_args` separately at
                // `raise_node_to_type_expr_inner`); the final reduced node is
                // driven through the demand reducer below.
                let (value_root, path) = data.typeof_head().expect("TypeOf carrier head");
                let value_root = value_root.clone();
                let path = Arc::clone(path);
                // Read the carrier args from the SAME borrow (owned copy so the
                // `data` borrow is not held across the apply call).
                let type_args: Vec<SemanticNodeId> = data.carrier_type_args().to_vec();

                // 1. Resolve the typeof value root.
                let typeof_key = self.typeof_key_for(value_root, context);
                let root_read = self.execute_read(typeof_key);
                state.merge_dep_signature(&root_read.dep_signature);
                if root_read.result_is_partial {
                    state.result_is_partial = true;
                    crate::request_context::mark_request_materialization_cache_suppress();
                }
                let root = match root_read.value {
                    QueryResult::Value(id) => id,
                    QueryResult::Recursive(_) | QueryResult::Error(_) => return node,
                };

                // 2. Project the carrier's dotted path (intermediate hops run in
                //    Navigate per the path-precision rule, mirroring evaluate).
                let projected = if path.is_empty() {
                    root
                } else {
                    let projection_path: Arc<[PathSegment]> = Arc::from(
                        path.iter()
                            .map(|segment| PathSegment::Member(Arc::clone(segment)))
                            .collect::<Vec<_>>()
                            .into_boxed_slice(),
                    );
                    let path_read = self.execute_read(SemanticQueryKey::ProjectPath {
                        base: root,
                        path: projection_path,
                        context: ProjectionReductionContext::published(ProjectionMode::Navigate),
                    });
                    state.merge_dep_signature(&path_read.dep_signature);
                    if path_read.result_is_partial {
                        state.result_is_partial = true;
                        crate::request_context::mark_request_materialization_cache_suppress();
                    }
                    match path_read.value {
                        QueryResult::Value(id) => id,
                        QueryResult::Recursive(_) | QueryResult::Error(_) => return node,
                    }
                };

                // 3. Apply the instantiation `type_args` to the projected
                //    signature. An arity/shape mismatch composes an honest
                //    `Opaque(Miss)` AFTER the projection.
                let final_node = if type_args.is_empty() {
                    projected
                } else {
                    self.apply_typeof_instantiation_args(projected, &type_args)
                };

                // 4. Drive the reduced node through the demand reducer (same
                //    result-threading as `dispatch_operator_with_recurse`):
                //    a self-identity result stays put; an already-reduced node
                //    is reused; otherwise its demanded children reduce under
                //    `context`.
                if final_node == node {
                    node
                } else if let Some(&already_reduced) = state.mapping.get(&(final_node, context)) {
                    already_reduced
                } else {
                    self.reduce_subtree(final_node, context, state)
                }
            }

            // --- lazy carriers ---
            SemanticNodeData::DeclRef { identity } => {
                if matches!(mode, ProjectionMode::Navigate)
                    && userland_instantiation_body_is_closed_object(self.ctx, identity)
                {
                    // A `Published(Navigate)` terminal that lands ON a
                    // closed-object declaration (a nominal interface)
                    // stays the declaration-reference carrier — the
                    // published shape is identical to writing the plain
                    // reference, and the consumer re-resolves it on
                    // demand (shallow-by-default). This mirrors the
                    // `InstantiationRef` closed-object carve-out below.
                    // Alias / operator-bodied declarations still
                    // resolve: aliases are semantically transparent and
                    // an operator body is the demanded reduction.
                    //
                    // The verdict consulted the declaring file's
                    // prepared body — root it on the active fact tracer
                    // so a body-shape edit invalidates the published
                    // entry.
                    observe_closedness_walk_consult(self.ctx, identity.canonical_id.as_ref());
                    return node;
                }
                // Navigate follows alias chains because aliases are
                // semantically transparent. Dispatch and recurse — same
                // as Expanded for DeclRef.
                let scope = ScopeId {
                    canonical_id: Arc::clone(&identity.canonical_id),
                    local_scope: None,
                };
                let key = SemanticQueryKey::ResolveDecl(ResolveDeclKey {
                    scope,
                    name: Arc::clone(&identity.decl_name),
                });
                self.dispatch_operator_with_recurse(node, key, context, state)
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                if matches!(context.demand, ReductionDemand::StructuralTransit) {
                    // Demand-driven: an InstantiationRef inspected by a
                    // structural-transit caller stays terminal —
                    // structural observation only; do not reify the
                    // body.
                    return node;
                }
                if matches!(mode, ProjectionMode::Navigate)
                    && base.canonical_id.as_ref() != "__builtin__"
                    && userland_instantiation_body_is_closed_object(self.ctx, base)
                {
                    // ChatMessages leak verdict: a
                    // userland `InstantiationRef` at a
                    // `Published(Navigate)` publication terminal
                    // STAYS TERMINAL **when its declared body is a
                    // closed Object surface** (a generic interface
                    // like `Tool<INPUT, OUTPUT> { outputSchema:
                    // ..., execute: ... }`). The earlier
                    // `Pub(Expanded)` hardcoding eagerly unwrapped
                    // these and fired
                    // `ProjectMember(outputSchema|execute)` audit
                    // edges that no caller demanded.
                    //
                    // Userland generic HELPERS whose body is
                    // operator-shaped (`Lookup<M, I> = M[I]`,
                    // `MyPick<X, K> = { [P in K]: X[P] }`, etc.)
                    // DO reduce even under Navigate —
                    // the type-arg substitution into an operator
                    // body is the "demanded instantiation is
                    // reduced as the terminal" case. Closed-object
                    // bodies behave like nominal interfaces, not
                    // operator helpers.
                    //
                    // Builtin utility types (`Pick`/`Omit`/...,
                    // `canonical_id == "__builtin__"`) do not match this
                    // closed-object carve-out — they are gated by the
                    // open-enumeration-domain carrier-stop (L1) below.
                    return node;
                }
                // L1 (Shallow-By-Default), route/mode-INDEPENDENT: an
                // object-filter utility (`Pick`/`Omit`) whose enumeration
                // domain (source argument) is OPEN — an unbound generic, an
                // open conditional/mapped/indexed/keyof, an instantiation
                // over open args, or an unresolved declaration — STAYS a
                // shallow carrier instead of materialising its source, in
                // EVERY demand context / mode. Materialising an open source
                // degenerates into full cross-file generic expansion (the
                // `ChatMessages.vue` `Pick<PropsBase<T>, …>` storm and the
                // `Table.vue` `Omit<CoreOptions<T>, …>` structural memo-cycle).
                // A CLOSED source (finite object surface / concrete
                // instantiation) still materialises path-precisely, in every
                // mode.
                //
                // The domain args are substituted through `mapping` first so a
                // bound type argument (a closed instantiation) is judged
                // closed and still reduces.
                let resolved_args: Vec<SemanticNodeId> = args
                    .iter()
                    .map(|id| state.mapping.get(&(*id, context)).copied().unwrap_or(*id))
                    .collect();
                if utility_enumeration_domain_is_open_or_unknown(self.ctx, base, &resolved_args) {
                    return node;
                }
                let base_key =
                    self.type_slot_for(Arc::clone(&base.canonical_id), Arc::clone(&base.decl_name));
                let inst_ctx = self.instantiate_context_for(&base.canonical_id, context);
                let args: Arc<[SemanticNodeId]> = Arc::from(
                    args.iter()
                        .map(|id| state.mapping.get(&(*id, context)).copied().unwrap_or(*id))
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                );
                let key = SemanticQueryKey::Instantiate {
                    base: base_key,
                    args,
                    context: inst_ctx,
                };
                self.dispatch_operator_with_recurse(node, key, context, state)
            }

            // --- composite rebuilds via intern_preserving_scope ---
            //
            // Composite children are only reduced under whole-surface
            // `Published(Expanded)` (demand traversal rule). For
            // per-prop `Published(Navigate)` / `Published(Shallow)` and
            // `StructuralTransit`, the composite parent is the demand
            // terminal — the `rebuild_*` helpers see no child entries
            // in `mapping` and return `node` unchanged.
            SemanticNodeData::Object(_) => {
                rebuild_object(self, node, &state.mapping, context).unwrap_or(node)
            }
            SemanticNodeData::Union(arms) => rebuild_union_or_intersection(
                self,
                node,
                arms,
                /* is_union */ true,
                &state.mapping,
                context,
            )
            .unwrap_or(node),
            SemanticNodeData::Intersection(arms) => rebuild_union_or_intersection(
                self,
                node,
                arms,
                /* is_union */ false,
                &state.mapping,
                context,
            )
            .unwrap_or(node),
            SemanticNodeData::Array { element, readonly } => {
                let new_elem = state
                    .mapping
                    .get(&(*element, context))
                    .copied()
                    .unwrap_or(*element);
                if new_elem == *element {
                    node
                } else {
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::Array {
                            element: new_elem,
                            readonly: *readonly,
                        },
                    )
                }
            }
            SemanticNodeData::Tuple { elements, readonly } => {
                rebuild_tuple(self, node, elements, *readonly, &state.mapping, context)
                    .unwrap_or(node)
            }
            SemanticNodeData::Function {
                params,
                return_type,
                type_parameters,
                signature_span,
                return_type_span,
            } => rebuild_function(
                self,
                node,
                params,
                *return_type,
                type_parameters,
                *signature_span,
                *return_type_span,
                &state.mapping,
                context,
            )
            .unwrap_or(node),
            SemanticNodeData::MergedDecl { contributors } => {
                // Reduce the peer-merged surface, then drive it through the
                // reducer so its demanded children reduce under `context`.
                let merged = self.reduce_merged_decl(contributors);
                if let Some(&already) = state.mapping.get(&(merged, context)) {
                    already
                } else {
                    self.reduce_subtree(merged, context, state)
                }
            }
            // Structural fidelity carriers rebuild from their reduced single
            // child (like `Array` / `Function`): if the child is unchanged
            // the shell is preserved, else a scope-preserving shell is
            // re-interned.
            SemanticNodeData::ConstructorType { signature } => {
                let new_sig = state
                    .mapping
                    .get(&(*signature, context))
                    .copied()
                    .unwrap_or(*signature);
                if new_sig == *signature {
                    node
                } else {
                    self.graph().intern_preserving_scope(
                        node,
                        SemanticNodeData::ConstructorType {
                            signature: new_sig,
                        },
                    )
                }
            }
        }
    }

    /// Helper: dispatch `key` and accumulate the dep_signature from the
    /// `CacheRead`.
    ///
    /// On `Value(result)`:
    /// - if `result == node` (deferred form), return `node` unchanged.
    /// - if `result != node`, recursively reduce `result` (it may itself
    ///   contain further operator nodes), then return that.
    ///
    /// On `Recursive(id)` or `Error(_)`: return `node` (deferred form).
    #[allow(dead_code)] // wired by reduce_one above.
    fn dispatch_operator_with_recurse(
        &self,
        node: SemanticNodeId,
        key: SemanticQueryKey,
        context: ProjectionReductionContext,
        state: &mut ReduceState,
    ) -> SemanticNodeId {
        // Loop-5 instrumentation — every operator-node dispatch issues
        // one `execute_read` (which routes through `execute_cooperative`).
        crate::loop5_instrumentation::DISPATCH_OPERATOR_WITH_RECURSE_CALLS
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Loop-7 instrumentation — per-`SemanticQueryKey`-variant
        // wall-clock attribution. The kind index is captured BEFORE
        // `execute_read` consumes the key. The wall-clock window
        // covers `execute_read` (warm-hit fast-path AND cold-build
        // close-down) AND any recursive `reduce_subtree` follow-up the
        // dispatch triggers — i.e. the entire wall-clock cost
        // attributable to this single operator-node dispatch.
        let kind_idx = crate::loop5_instrumentation::kind_index_for_key(&key);
        crate::loop5_instrumentation::DISPATCH_OPERATOR_KIND_CALLS[kind_idx]
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dispatch_started = Instant::now();

        let read = self.execute_read(key);
        state.merge_dep_signature(&read.dep_signature);
        // Partial-result propagation: a `result_is_partial=true` read
        // means the semantic dispatch produced a PARTIAL outcome (budget
        // exhaustion / cancellation / same-path recursion / walker
        // fatal). Such a result must not warm any shared cache. Mark the
        // per-reduce-state flag so the enclosing
        // `raise_and_reduce_with_context` returns a `MaterializedTypeExpr`
        // carrying `result_is_partial=true`, AND raise the request-scoped
        // sticky bit so downstream callers (the projector second pass, the
        // final ComponentMeta cache admission gate) observe it without
        // needing a hand-threaded return value. A benign non-cacheable
        // read (`cache_suppress` without partiality — ReturnOnly /
        // overflow / unrootable self-root) is NOT folded here: it refuses
        // only its own inner-memo admission and MUST NOT suppress a
        // complete component-meta result.
        if read.result_is_partial {
            state.result_is_partial = true;
            crate::request_context::mark_request_materialization_cache_suppress();
        }
        let result = match read.value {
            QueryResult::Value(result) => {
                if result == node {
                    node
                } else if let Some(&already_reduced) = state.mapping.get(&(result, context)) {
                    already_reduced
                } else {
                    // The dispatch produced a new node not yet in
                    // `mapping`. Drive it through the iterative
                    // reducer's worklist so its demanded children are
                    // selectively reduced under `context` (the
                    // demand-traversal rule). Cycle protection: the
                    // shared `visited` set deduplicates re-entry.
                    self.reduce_subtree(result, context, state)
                }
            }
            QueryResult::Recursive(_id) => node,
            QueryResult::Error(_) => node,
        };

        let elapsed_ns = dispatch_started.elapsed().as_nanos() as u64;
        crate::loop5_instrumentation::DISPATCH_OPERATOR_KIND_NS[kind_idx]
            .fetch_add(elapsed_ns, std::sync::atomic::Ordering::Relaxed);
        crate::loop5_instrumentation::DISPATCH_OPERATOR_TOTAL_NS
            .fetch_add(elapsed_ns, std::sync::atomic::Ordering::Relaxed);
        result
    }

    /// Helper: convert a reducer-driven hard-stop into an
    /// `Opaque(QueryError::Other(reason))` interned at the origin node's
    /// scope so subsequent raises render the documented sentinel.
    #[allow(dead_code)] // wired by reduce_one above.
    fn opaque_unknown_with(&self, origin: SemanticNodeId, reason: &str) -> SemanticNodeId {
        self.graph().intern_preserving_scope(
            origin,
            SemanticNodeData::Opaque(QueryError::Other(Arc::from(reason))),
        )
    }
}

/// `(SemanticNodeId, ProjectionReductionContext)` → `SemanticNodeId`
/// map used by the reducer to fetch already-reduced operand / child
/// reductions. Keyed by the operand's own context so a structural-
/// transit reduction does not collide with a publication reduction of
/// the same node.
type MappingMap =
    rustc_hash::FxHashMap<(SemanticNodeId, ProjectionReductionContext), SemanticNodeId>;

/// Top-down demand reducer work-stack frame.
#[derive(Clone, Copy)]
#[allow(dead_code)] // wired by reduce_subtree.
enum ReduceFrameKind {
    /// Mark `(node, context)` visited, push the matching `Reduce`
    /// frame, then push the demand-selected children.
    Descend,
    /// All demand-selected children have been reduced into
    /// `state.mapping`; invoke `reduce_one(node, context, state)` and
    /// record the result.
    Reduce,
}

#[allow(dead_code)] // wired by reduce_subtree.
struct ReduceFrame {
    node: SemanticNodeId,
    context: ProjectionReductionContext,
    kind: ReduceFrameKind,
}

#[allow(dead_code)] // wired by reduce_subtree.
impl ReduceFrame {
    #[inline]
    fn descend(node: SemanticNodeId, context: ProjectionReductionContext) -> Self {
        Self {
            node,
            context,
            kind: ReduceFrameKind::Descend,
        }
    }
}

/// `true` when `ctx` is the demand-driven whole-surface publication
/// demand (`Published + Expanded`). Composite-child traversal pushes
/// descend frames ONLY in this case; per-prop `Published(Navigate)` /
/// `Published(Shallow)` and any `StructuralTransit` walk treat the
/// composite parent as the demand terminal.
#[allow(dead_code)] // wired by push_demand_children + child-context helpers.
#[inline]
fn is_whole_surface_published(ctx: ProjectionReductionContext) -> bool {
    matches!(ctx.demand, ReductionDemand::Published) && matches!(ctx.mode, ProjectionMode::Expanded)
}

/// Derive the `IndexedAccess.object` operand context from the parent
/// IndexedAccess's reduction context.
///
/// The demand-driven reducer spec ("`Foo['a']['b']`: intermediate hops are
/// navigate-only, terminal uses caller mode") describes the
/// PATH-WALK semantics inside the dispatch's `ProjectPath` builder.
/// At the iterative-reducer layer the object operand must be REDUCED
/// enough for the `IndexedAccess` dispatch to look up the index — so
/// a generic instantiation like `Pick<Foo, 'a'>` materialises its
/// body and the indexed access can pick `'a'` out of it.
///
/// Under any `Published` parent → demote the object operand to
/// [`ProjectionMode::Navigate`] (demand + provenance + merge_role
/// preserved). The object operand is the INTERMEDIATE hop of the
/// indexed-access path (`Root['a']` in `Root['a']['b']`): it must be
/// navigated (followed through aliases / generic bodies so the outer
/// `IndexedAccess` dispatch can look up the index) but NOT expanded.
/// Only the OUTER `IndexedAccess` dispatch keeps the caller's mode, so
/// the terminal consumed segment is the sole one that expands. This is
/// the path-precision rule "intermediate hops run in Navigate, the
/// terminal hop runs in the caller's mode" applied to the
/// raise/materialize reducer. Inheriting the parent's mode here would
/// over-expand the intermediate object (e.g. materialise the sibling
/// members of `Root['a']` that the path never selects) — the
/// shallow-by-default violation this demotion fixes.
///
/// Under `StructuralTransit` parent → `StructuralTransit` (the transit
/// walk observes the object structurally without materialising it).
#[allow(dead_code)] // wired by push_demand_children + reduce_one IndexedAccess.
#[inline]
fn indexed_access_object_context(
    parent_context: ProjectionReductionContext,
) -> ProjectionReductionContext {
    if matches!(parent_context.demand, ReductionDemand::Published) {
        parent_context.with_mode(ProjectionMode::Navigate)
    } else {
        ProjectionReductionContext::structural_transit()
    }
}

/// Re-key a `MapperKey` using `mapping` (substituting any reduced
/// `key_space` id from the demand walk). The mapper's `value_expr` /
/// `name_remap` / `parameter_node` are NOT looked up — the dispatch
/// substitutes them per-key internally under a structural-transit
/// evaluation (see [`crate::project_semantic_dispatch::evaluate`]'s
/// context-explicit variant).
#[allow(dead_code)] // wired by reduce_one above.
fn remap_mapper(
    mapper: &MapperKey,
    mapping: &MappingMap,
    source_context: ProjectionReductionContext,
) -> MapperKey {
    let mut new_mapper = mapper.clone();
    new_mapper.key_space = mapping
        .get(&(mapper.key_space, source_context))
        .copied()
        .unwrap_or(mapper.key_space);
    new_mapper
}

#[allow(dead_code)] // wired by reduce_one above.
fn rebuild_object(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    mapping: &MappingMap,
    context: ProjectionReductionContext,
) -> Option<SemanticNodeId> {
    let data = super::node_data_for(dispatch.ctx, node)?;
    let SemanticNodeData::Object(view) = data.as_ref() else {
        return None;
    };
    let mut changed = false;
    let new_members: Arc<[SurfaceMember]> = {
        let mut out: Vec<SurfaceMember> = Vec::with_capacity(view.members.len());
        for m in view.members.iter() {
            let new_value = mapping.get(&(m.value, context)).copied().unwrap_or(m.value);
            if new_value != m.value {
                changed = true;
            }
            out.push(SurfaceMember {
                value: new_value,
                ..m.clone()
            });
        }
        Arc::from(out.into_boxed_slice())
    };
    let new_calls: Arc<[SemanticNodeId]> = {
        let mut out = Vec::with_capacity(view.call_signatures.len());
        for sig in view.call_signatures.iter() {
            let new_sig = mapping.get(&(*sig, context)).copied().unwrap_or(*sig);
            if new_sig != *sig {
                changed = true;
            }
            out.push(new_sig);
        }
        Arc::from(out.into_boxed_slice())
    };
    let new_constructs: Arc<[SemanticNodeId]> = {
        let mut out = Vec::with_capacity(view.construct_signatures.len());
        for sig in view.construct_signatures.iter() {
            let new_sig = mapping.get(&(*sig, context)).copied().unwrap_or(*sig);
            if new_sig != *sig {
                changed = true;
            }
            out.push(new_sig);
        }
        Arc::from(out.into_boxed_slice())
    };
    if !changed {
        return Some(node);
    }
    let new_view = SurfaceView {
        members: new_members,
        call_signatures: new_calls,
        construct_signatures: new_constructs,
        index_signatures: view.index_signatures.clone(),
        keyspace: view.keyspace,
        has_index_signature: view.has_index_signature,
    };
    Some(
        dispatch
            .graph()
            .intern_preserving_scope(node, SemanticNodeData::Object(new_view)),
    )
}

#[allow(dead_code)] // wired by reduce_one above.
fn rebuild_union_or_intersection(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    arms: &Arc<[SemanticNodeId]>,
    is_union: bool,
    mapping: &MappingMap,
    context: ProjectionReductionContext,
) -> Option<SemanticNodeId> {
    let mut changed = false;
    let new_arms: Vec<SemanticNodeId> = arms
        .iter()
        .map(|arm| {
            let new = mapping.get(&(*arm, context)).copied().unwrap_or(*arm);
            if new != *arm {
                changed = true;
            }
            new
        })
        .collect();
    if !changed {
        return Some(node);
    }
    let data = if is_union {
        SemanticNodeData::Union(Arc::from(new_arms.into_boxed_slice()))
    } else {
        SemanticNodeData::Intersection(Arc::from(new_arms.into_boxed_slice()))
    };
    Some(dispatch.graph().intern_preserving_scope(node, data))
}

#[allow(dead_code)] // wired by reduce_one above.
fn rebuild_tuple(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    elements: &Arc<[TupleElement]>,
    readonly: bool,
    mapping: &MappingMap,
    context: ProjectionReductionContext,
) -> Option<SemanticNodeId> {
    let mut changed = false;
    let new_elements: Vec<TupleElement> = elements
        .iter()
        .map(|el| {
            let new_value = mapping
                .get(&(el.value, context))
                .copied()
                .unwrap_or(el.value);
            if new_value != el.value {
                changed = true;
            }
            TupleElement {
                value: new_value,
                ..el.clone()
            }
        })
        .collect();
    if !changed {
        return Some(node);
    }
    Some(dispatch.graph().intern_preserving_scope(
        node,
        SemanticNodeData::Tuple {
            elements: Arc::from(new_elements.into_boxed_slice()),
            readonly,
        },
    ))
}

#[allow(dead_code)] // wired by reduce_one above.
#[allow(clippy::too_many_arguments)]
fn rebuild_function(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    params: &Arc<[crate::semantic_query::FunctionParam]>,
    return_type: SemanticNodeId,
    type_parameters: &Arc<[crate::semantic_query::TypeParamDecl]>,
    signature_span: Option<verter_span::Span>,
    return_type_span: Option<verter_span::Span>,
    mapping: &MappingMap,
    context: ProjectionReductionContext,
) -> Option<SemanticNodeId> {
    let mut changed = false;
    let new_params: Vec<crate::semantic_query::FunctionParam> = params
        .iter()
        .map(|p| {
            let new_ty = mapping.get(&(p.ty, context)).copied().unwrap_or(p.ty);
            if new_ty != p.ty {
                changed = true;
            }
            crate::semantic_query::FunctionParam {
                ty: new_ty,
                ..p.clone()
            }
        })
        .collect();
    let new_return = mapping
        .get(&(return_type, context))
        .copied()
        .unwrap_or(return_type);
    if new_return != return_type {
        changed = true;
    }
    let new_type_params: Vec<crate::semantic_query::TypeParamDecl> = type_parameters
        .iter()
        .map(|tp| {
            let new_constraint = tp
                .constraint
                .map(|c| mapping.get(&(c, context)).copied().unwrap_or(c));
            let new_default = tp
                .default
                .map(|d| mapping.get(&(d, context)).copied().unwrap_or(d));
            if new_constraint != tp.constraint || new_default != tp.default {
                changed = true;
            }
            crate::semantic_query::TypeParamDecl {
                constraint: new_constraint,
                default: new_default,
                ..tp.clone()
            }
        })
        .collect();
    if !changed {
        return Some(node);
    }
    Some(dispatch.graph().intern_preserving_scope(
        node,
        SemanticNodeData::Function {
            params: Arc::from(new_params.into_boxed_slice()),
            return_type: new_return,
            type_parameters: Arc::from(new_type_params.into_boxed_slice()),
            // Node remapping preserves source provenance.
            signature_span,
            return_type_span,
        },
    ))
}

/// Materialization-grade result returned by [`raise_and_reduce`] and the
/// session-layer `materialize_*` wrapper.
///
/// The session-layer caller `materialize_component_meta_type_expr_until_stable`
/// returns this struct.
///
/// - `node_id`: `Some(id)` for dispatch-produced entries; `None` for
///   synthetic / inline-annotation entries that bypass the dispatch
///   path. Captured at materialization time for `SurfaceNodeIdentities`
///   population.
/// - `type_expr`: the raised final form.
/// - `dep_signature`: accumulated fence signatures from all dispatch
///   calls inside reduction. Session merges into
///   `ResolvedComponentMetaState.fact_versions` before publish.
#[derive(Debug, Clone)]
pub struct MaterializedTypeExpr {
    pub node_id: Option<SemanticNodeId>,
    pub type_expr: TypeExpr,
    pub dep_signature: DepSignature,
    /// `true` when ANY semantic-dispatch read consumed by the reducer
    /// returned a PARTIAL value (`result_is_partial` — projection-budget
    /// exhaustion, cancellation, same-path recursion, or a walker
    /// fatal/pathological diagnostic). Callers that publish the
    /// materialized result into a downstream shared cache (e.g. the
    /// per-field cache in `field_types.rs`, the projector second pass)
    /// must propagate this bit so the final-result `ComponentMetaResultDb`
    /// admission gate observes it and refuses to warm a partial. A benign
    /// non-cacheable nested read (`cache_suppress` without `result_is_partial`
    /// — ReturnOnly / overflow / unrootable self-root) does NOT set this:
    /// a complete-but-non-cacheable result MUST still be allowed to warm
    /// the component-meta result.
    pub result_is_partial: bool,
}

#[allow(dead_code)] // wired by raise_and_reduce above.
#[derive(Default)]
struct ReduceState {
    visited: FxHashSet<(SemanticNodeId, ProjectionReductionContext)>,
    mapping: MappingMap,
    dep_facts: Vec<(Arc<str>, crate::semantic_query::DepVersion)>,
    /// OR-fold of every `read.result_is_partial` observed by `reduce_one`
    /// during this reduce pass. Propagated into the returned
    /// `MaterializedTypeExpr.result_is_partial` so direct consumers
    /// (e.g. `field_types::materialize_component_meta_type_expr_until_stable_full`)
    /// can refuse to publish a PARTIAL result into shared caches without
    /// needing to inspect the request-scoped TLS flag. NOT set by a
    /// benign non-cacheable read (`cache_suppress` without partiality).
    result_is_partial: bool,
}

#[allow(dead_code)] // wired by raise_and_reduce above.
impl ReduceState {
    fn merge_dep_signature(&mut self, sig: &DepSignature) {
        for (canonical, version) in sig.iter() {
            // Light dedup on (canonical, version) — keeps the
            // accumulated list O(unique_facts) rather than
            // O(num_dispatches × facts_per_dispatch).
            if !self
                .dep_facts
                .iter()
                .any(|(c, v)| Arc::ptr_eq(c, canonical) && v == version)
                && !self
                    .dep_facts
                    .iter()
                    .any(|(c, v)| c.as_ref() == canonical.as_ref() && v == version)
            {
                self.dep_facts
                    .push((Arc::clone(canonical), version.clone()));
            }
        }
    }

    fn into_dep_signature(self) -> DepSignature {
        Arc::from(self.dep_facts.into_boxed_slice())
    }
}

/// Inspect a userland decl's prepared body to decide whether its
/// `InstantiationRef` should stay terminal under
/// `Published(Navigate)`. The discriminator:
///
/// - The prepared body's top-level kind is an `Object` /
///   `Intersection of Objects` / closed nominal interface → the
///   instantiation is a NOMINAL generic interface; stays terminal.
/// - The body is operator-shaped (`IndexedAccess`, `KeyOf`,
///   `Mapped`, `Conditional`, `IndexedAccess` chain, etc.) → the
///   instantiation is a generic HELPER that materially substitutes
///   type args; reduces even under Navigate.
///
/// When the body cannot be peeked cheaply (no prepared decl,
/// resolution miss), the function returns `false` — the reducer
/// proceeds with the `Instantiate` dispatch as the safe default
/// (matches the earlier behaviour).
#[allow(dead_code)] // wired by InstantiationRef Navigate-terminal gate.
fn userland_instantiation_body_is_closed_object(
    ctx: &dyn crate::resolver_core::ResolverContext,
    base: &crate::semantic_query::DeclIdentity,
) -> bool {
    let prepared = match ctx.prepared_type_decl(base.canonical_id.as_ref(), base.decl_name.as_ref())
    {
        Some(prepared) => prepared,
        None => return false,
    };
    is_closed_object_body(&prepared.body)
}

/// Inspect a `TypeExpr` body to decide whether it's a "closed
/// nominal" Object surface. Used by
/// [`userland_instantiation_body_is_closed_object`].
fn is_closed_object_body(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Object(_) => true,
        TypeExpr::Intersection(arms) => arms.iter().all(is_closed_object_body),
        TypeExpr::Parenthesized(inner) => is_closed_object_body(inner),
        _ => false,
    }
}

/// Root one cross-file consult of the closedness walk on the active fact
/// tracer.
///
/// The openness/closedness predicates read OTHER canonical files than the
/// query's own inputs: transparent alias-chain hops, barrel re-export hops,
/// and prepared-decl bodies (`prepared_decl_body_is_closed` /
/// `prepared_instantiation_key_domain_is_closed`). The verdict — and
/// therefore the carrier-vs-materialise shape of the published value —
/// depends on every consulted file's content, so each consult must enter
/// the published entry's `ReadSetSignature.facts`: an edit to a chain file
/// then rejects the warm entry on the read-side validator and the verdict
/// recomputes (the read-side-authoritative cache rule). Emission rides the
/// SAME `observe_fan_out` rail the module-augmentation stitch uses
/// (`collect_augmentation_contributions`), so ANY enclosing
/// `install_fact_tracer` scope — the dispatch memo cold build, the
/// component-meta result compute, the materialise producer — picks the
/// fact up without route-specific plumbing; the rooting is
/// route/mode-independent like the carrier-stop itself.
///
/// The observed hash is the consult-time `IndexedReady.whole_hash` (one
/// atomic observation — never a separate current-content re-read). Non-file
/// canonicals (the builtin / synthetic / empty sentinels) and files unknown
/// to the live view observe nothing.
fn observe_closedness_walk_consult(
    ctx: &dyn crate::resolver_core::ResolverContext,
    canonical_id: &str,
) {
    if canonical_id.is_empty() || canonical_id == "__builtin__" || canonical_id == "<synthetic>" {
        return;
    }
    // Structurally read-only: observes the consulted file's whole hash
    // onto the active tracer; fenced-ness flows via the chokepoint flag.
    if let Some(indexed) = ctx
        .ensure_indexed_ready_serve(canonical_id)
        .map(|serve| serve.indexed)
    {
        crate::resolver_core::resolver_context::observe_fan_out(
            crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: canonical_id.to_string(),
                hash: indexed.whole_hash,
            },
        );
    }
}

/// Node budget for the bounded TypeExpr key-domain walk
/// ([`key_domain_type_expr_is_closed`] + the decl hops of
/// [`prepared_decl_body_is_closed`]). Every inspected node (and every
/// transparent alias / barrel hop) decrements it; a legitimate key domain
/// — alias chains, heritage arms, finite key-literal unions — resolves
/// well inside the cap. Exhaustion ⇒ conservatively OPEN.
const KEY_DOMAIN_TYPE_EXPR_WALK_BUDGET: u32 = 256;

/// Identity-preserving binding for ONE type-parameter slot of the
/// key-domain walks — the replacement for the bool-only `open_args`
/// environment. Beyond the open/closed verdict, a closed binding carries
/// the ACTUAL bound value (`TypeExpr` or `SemanticNodeId`) where one is
/// scope-safely available, so the conditional branch-selection oracle can
/// resolve a check/extends operand that references a parameter bound to a
/// concrete argument — even while another (losing) branch contains an
/// open parameter.
#[derive(Clone, Copy)]
enum KeyDomainBinding<'e> {
    /// Bound to an argument the walk judged OPEN.
    Open,
    /// Closed with no concrete identity: a mapper binder, an
    /// `infer`-introduced name, an unfilled defaulted parameter, or a
    /// closed argument whose identity could not be normalised
    /// scope-safely.
    ClosedAbstract,
    /// Closed, bound to an ENVIRONMENT-FREE `TypeExpr` (literals /
    /// primitives / parenthesized chains of those) — safe to consult
    /// across declaration scopes because it resolves no names.
    ClosedExpr(&'e TypeExpr),
    /// Closed, bound to an interned semantic node (the node route's
    /// argument identity — scope-free by construction).
    ClosedNode(SemanticNodeId),
}

impl KeyDomainBinding<'_> {
    fn is_open(self) -> bool {
        matches!(self, KeyDomainBinding::Open)
    }
}

/// The TypeExpr-layer binding environment: declared type-parameter name →
/// identity-preserving binding. An ABSENT name is a FREE parameter
/// (open); a present binding is open or closed per
/// [`KeyDomainBinding`].
type KeyDomainBindings<'e> = FxHashMap<&'e str, KeyDomainBinding<'e>>;

/// Operand-position policy axis shared by the TypeExpr classifier
/// ([`key_domain_type_expr_is_closed`]) and the node-level [`OpenWalk`].
///
/// The per-argument key-domain rule (an open argument confined to member
/// VALUE positions of a fixed-key body keeps the key domain CLOSED) is
/// sound only where the enclosing expression consumes the operand's KEY
/// SET. Where the enclosing operator consumes the operand's VALUES —
/// `Conditional.check` / `Conditional.extends` (branch selection relates
/// the operand's value structure) and `IndexedAccess.object` (the access
/// projects a member VALUE out of the object) — an instantiation is OPEN
/// if ANY argument is open. Conditional BRANCHES stay in the surrounding
/// position; `IndexedAccess.index` remains a key/keyspace question
/// (`KeyDomain`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum OperandPosition {
    /// A genuine KEY-DOMAIN position (`Pick`/`Omit` source, mapped
    /// source/keyspace, indexed-access index): instantiations are judged
    /// by the per-argument key-domain rule.
    KeyDomain,
    /// A VALUE-SENSITIVE operand (`Conditional.check`,
    /// `Conditional.extends`, `IndexedAccess.object`): an instantiation
    /// is OPEN if ANY argument is open.
    ValueSensitive,
}

/// Whether `expr` resolves NO names — literals, primitives, and
/// parenthesized chains of those. Environment-free exprs are the only
/// `TypeExpr` values a [`KeyDomainBinding::ClosedExpr`] may carry across
/// declaration scopes (anything else would need the ORIGINATING decl's
/// `name_resolution` to mean the same thing elsewhere).
fn type_expr_is_environment_free(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Literal(_) | TypeExpr::Primitive(_) => true,
        TypeExpr::Parenthesized(inner) => type_expr_is_environment_free(inner),
        _ => false,
    }
}

/// Normalise a CLOSED instantiation argument (or a verified-closed
/// parameter DEFAULT) into its identity-preserving binding.
/// Environment-free shapes keep their `TypeExpr` identity; a closed
/// reference to a currently-bound parameter FORWARDS that parameter's
/// own binding (a wrapper hop — or a `B = A` param-ref default —
/// preserves the original argument identity); a closed NAMED type
/// resolves in ITS OWN originating scope (`prepared.name_resolution`)
/// to an interned `DeclRef` node identity so the branch-selection
/// oracle can relate it; any other closed shape — scope-dependent
/// structure — degrades to [`KeyDomainBinding::ClosedAbstract`]: still
/// closed, no identity. Callers must have ALREADY judged `arg` closed.
fn normalise_closed_arg_binding<'e>(
    ctx: &dyn crate::resolver_core::ResolverContext,
    prepared: &'e verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl,
    arg: &'e TypeExpr,
    bindings: &KeyDomainBindings<'e>,
) -> KeyDomainBinding<'e> {
    if type_expr_is_environment_free(arg) {
        return KeyDomainBinding::ClosedExpr(arg);
    }
    match arg {
        TypeExpr::TypeParameter(param) => bindings
            .get(param.name.as_str())
            .copied()
            .unwrap_or(KeyDomainBinding::ClosedAbstract),
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => match bindings.get(name.as_ref()) {
            Some(binding) => *binding,
            // A closed NAMED type used as an actual: carry its resolved
            // identity (never a foreign scope's name table — resolution
            // is the prepared decl's own `name_resolution`). Unresolvable
            // ⇒ ClosedAbstract (closed, no identity — the oracle stays
            // Deferred, the honest fallback).
            None => resolve_closed_named_ref_operand_node(ctx, prepared, name.as_ref())
                .map(KeyDomainBinding::ClosedNode)
                .unwrap_or(KeyDomainBinding::ClosedAbstract),
        },
        TypeExpr::Parenthesized(inner) => {
            normalise_closed_arg_binding(ctx, prepared, inner, bindings)
        }
        _ => KeyDomainBinding::ClosedAbstract,
    }
}

/// Resolve a CLOSED bare named reference appearing inside `prepared`'s
/// body to an interned [`SemanticNodeData::DeclRef`] node for the
/// branch-selection oracle. Resolution happens in the reference's OWN
/// originating scope (`prepared.name_resolution` — the same
/// prepared-decl identity machinery the closedness walk hops aliases
/// with), and the consult is rooted on the active fact tracer (the
/// selection verdict depends on the target file's content). Returns
/// `None` when the name or its file state is unavailable — selection
/// stays `Deferred`, the honest fallback. No reduction, no private
/// assignability: the node is an identity carrier the ONE relation
/// engine resolves.
fn resolve_closed_named_ref_operand_node(
    ctx: &dyn crate::resolver_core::ResolverContext,
    prepared: &verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl,
    name: &str,
) -> Option<SemanticNodeId> {
    let target = prepared.name_resolution.get(name)?;
    observe_closedness_walk_consult(ctx, &target.canonical_id);
    let state = ctx.shallow_file_state(&target.canonical_id)?;
    Some(
        ctx.project_type_store()
            .semantic_graph()
            .intern_node(SemanticNodeData::DeclRef {
                identity: crate::semantic_query::DeclIdentity {
                    canonical_id: Arc::from(target.canonical_id.as_str()),
                    whole_hash: state.whole_hash,
                    decl_name: Arc::from(target.symbol_name.as_str()),
                },
            }),
    )
}

/// Intern an ENVIRONMENT-FREE operand `TypeExpr` as a semantic node for
/// the branch-selection oracle. Literals and primitives intern directly
/// (the hash-consed graph gives identical operands the same node id, so
/// `true extends true` short-circuits on node identity in
/// `shallow_relation_check`). Anything else returns `None` — the
/// classifier NEVER lowers scope-dependent structure here.
fn environment_free_operand_node(
    ctx: &dyn crate::resolver_core::ResolverContext,
    expr: &TypeExpr,
) -> Option<SemanticNodeId> {
    let graph = ctx.project_type_store().semantic_graph();
    match expr {
        TypeExpr::Literal(value) => {
            Some(graph.intern_node(SemanticNodeData::Literal(value.clone())))
        }
        TypeExpr::Primitive(name) => Some(graph.intern_node(SemanticNodeData::Primitive(
            super::map_primitive_name(*name),
        ))),
        TypeExpr::Parenthesized(inner) => environment_free_operand_node(ctx, inner),
        _ => None,
    }
}

/// Resolve a conditional check/extends OPERAND `TypeExpr` to an interned
/// semantic node for the shared branch-selection oracle: environment-free
/// shapes intern directly; `infer X` interns as its scope-free binder
/// placeholder (so the oracle's pre-relation bare-infer case can see
/// it); a reference to a bound parameter resolves through its identity
/// binding ([`KeyDomainBinding::ClosedNode`] verbatim;
/// [`KeyDomainBinding::ClosedExpr`] is environment-free by
/// construction); an UNBOUND bare name resolves as a closed NAMED
/// reference in `prepared`'s own scope
/// ([`resolve_closed_named_ref_operand_node`]). Anything else —
/// open/abstract bindings, unresolvable names, composite structure —
/// returns `None` (selection `Deferred`).
fn conditional_selection_operand_node(
    ctx: &dyn crate::resolver_core::ResolverContext,
    prepared: &verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl,
    expr: &TypeExpr,
    bindings: &KeyDomainBindings<'_>,
) -> Option<SemanticNodeId> {
    match expr {
        TypeExpr::Literal(_) | TypeExpr::Primitive(_) => environment_free_operand_node(ctx, expr),
        TypeExpr::Parenthesized(inner) => {
            conditional_selection_operand_node(ctx, prepared, inner, bindings)
        }
        TypeExpr::Infer { name } => Some(ctx.project_type_store().semantic_graph().intern_node(
            SemanticNodeData::Infer {
                name: Arc::from(name.as_str()),
            },
        )),
        TypeExpr::TypeParameter(param) => {
            binding_selection_operand_node(ctx, bindings.get(param.name.as_str()).copied())
        }
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => match bindings.get(name.as_ref()).copied() {
            // A bound parameter / mapper binder / infer name resolves
            // ONLY through its binding (an Open / ClosedAbstract binding
            // must NOT fall through to declaration resolution).
            Some(binding) => binding_selection_operand_node(ctx, Some(binding)),
            None => resolve_closed_named_ref_operand_node(ctx, prepared, name.as_ref()),
        },
        _ => None,
    }
}

/// Identity half of [`conditional_selection_operand_node`]: a bound
/// parameter's binding resolves to a node only when it actually carries
/// one.
fn binding_selection_operand_node(
    ctx: &dyn crate::resolver_core::ResolverContext,
    binding: Option<KeyDomainBinding<'_>>,
) -> Option<SemanticNodeId> {
    match binding? {
        KeyDomainBinding::ClosedNode(id) => Some(id),
        KeyDomainBinding::ClosedExpr(expr) => environment_free_operand_node(ctx, expr),
        KeyDomainBinding::Open | KeyDomainBinding::ClosedAbstract => None,
    }
}

/// TypeExpr-layer tri-state conditional branch selection: resolve both
/// operands to nodes (identity-preserving — environment-free interning,
/// identity bindings, own-scope named-ref resolution; no private
/// lowering, no branch materialisation), then consult the ONE shared
/// oracle
/// ([`ProjectSemanticDispatch::conditional_branch_selection`]
/// — the pre-relation infer-pattern cases, `shallow_relation_check`,
/// then the full memoised `relate_nodes`; the SAME path
/// `build_conditional` selects branches with, so classification and
/// reduction cannot diverge). Returns the infer-pattern payload
/// alongside the selection so the caller can bind the selected branch's
/// infer names. A check operand that does not resolve still consults
/// the CHECK-INDEPENDENT pre-relation case (a bare-infer extends
/// selects TRUE for ANY check); otherwise an unresolved operand ⇒
/// `Deferred` (both branches classified).
fn type_expr_conditional_branch_selection(
    ctx: &dyn crate::resolver_core::ResolverContext,
    prepared: &verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl,
    check: &TypeExpr,
    extends: &TypeExpr,
    bindings: &KeyDomainBindings<'_>,
) -> (
    super::ConditionalBranchSelection,
    Option<super::InferPatternSelection>,
) {
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let extends_node = conditional_selection_operand_node(ctx, prepared, extends, bindings);
    let check_node = conditional_selection_operand_node(ctx, prepared, check, bindings);
    match (check_node, extends_node) {
        (Some(check_node), Some(extends_node)) => {
            dispatch.conditional_branch_selection(check_node, extends_node)
        }
        // The check operand does not resolve — an unresolvable check can
        // never be the `any`/`error` lattice extreme (those are
        // environment-free primitives, which always resolve), so only
        // the check-independent bare-infer case can still select; the
        // relation ladder and the function-infer case need a resolved
        // check.
        (None, Some(extends_node)) => {
            match dispatch.pre_relation_infer_selection(None, extends_node) {
                Some(selected) => (super::ConditionalBranchSelection::True, Some(selected)),
                None => (super::ConditionalBranchSelection::Deferred, None),
            }
        }
        _ => (super::ConditionalBranchSelection::Deferred, None),
    }
}

/// Whether the prepared declaration `(canonical, name)` resolves to a
/// surface with a CLOSED enumerable key domain through a BOUNDED walk.
///
/// Replaces the previous single-hop `is_closed_object_body(&prepared.body)`
/// shortcut, which mis-classified a perfectly legitimate closed alias chain
/// (`type Foo = Bar; type Bar = { bar: string }`) as OPEN — the source of
/// `Pick<Foo, 'bar'>` failing to materialise. The decl body is judged by
/// the ONE bound-param-aware classifier
/// ([`key_domain_type_expr_is_closed`]) under an EMPTY binding environment
/// (a bare-decl reference binds nothing); transparent alias hops resolve
/// through the prepared decl's `name_resolution` context (NO route
/// discovery, NO reducer, NO `execute_read` — prepared declaration cache
/// reads ONLY), bounded by a node budget + an in-flight cycle guard.
///
/// CLOSED ⇒ the walk proves a finite key domain (a finite object surface,
/// a finite union/intersection of those, a concrete operator body whose
/// operands all close). OPEN ⇒ the walk reaches a free type parameter, an
/// unresolved name, a missing prepared decl, a genuine cycle, or exhausts
/// the budget.
fn prepared_decl_body_is_closed(
    ctx: &dyn crate::resolver_core::ResolverContext,
    canonical_id: &str,
    decl_name: &str,
    budget: &mut u32,
    visited: &mut FxHashSet<(Arc<str>, Arc<str>)>,
) -> bool {
    let key = (Arc::<str>::from(canonical_id), Arc::<str>::from(decl_name));
    // IN-FLIGHT cycle guard, NOT a permanent visited set: the key is
    // removed on exit, so two SIBLING references to the same decl (a
    // diamond — `Foo & Bar` both reaching `Shared`) are each judged on
    // their merits; only a genuine back-edge on the current walk frontier
    // (true recursion) reports not-closed.
    if !visited.insert(key.clone()) {
        return false;
    }
    let verdict =
        prepared_decl_body_is_closed_unguarded(ctx, canonical_id, decl_name, budget, visited);
    visited.remove(&key);
    verdict
}

/// Cycle-unguarded core of [`prepared_decl_body_is_closed`] — never call
/// directly; the in-flight insert/remove discipline lives in the wrapper.
fn prepared_decl_body_is_closed_unguarded(
    ctx: &dyn crate::resolver_core::ResolverContext,
    canonical_id: &str,
    decl_name: &str,
    budget: &mut u32,
    visited: &mut FxHashSet<(Arc<str>, Arc<str>)>,
) -> bool {
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    // Root this cross-file consult (decl body OR barrel hop below) on the
    // active fact tracer — the closedness verdict depends on this file's
    // content, so an edit here must reject the consuming warm entry.
    observe_closedness_walk_consult(ctx, canonical_id);

    let Some(prepared) = ctx.prepared_type_decl(canonical_id, decl_name) else {
        // No local declaration at `(canonical, name)` — the name may be a
        // barrel RE-EXPORT (`export { LinkProps } from './link'`) rather than
        // a declaration in this file. Follow the re-export hop through the
        // shallow export map (cache reads only, no reducer / no execute_read)
        // and recurse on the resolved source decl, bounded by the same budget
        // + visited set. Without this a `Pick`/`Omit` over a barrel-reexported
        // CLOSED interface is mis-classified OPEN (the prepared decl lives in
        // the source file, not the barrel) and the L1 carrier-stop wrongly
        // fires on a genuinely-closed cross-file source.
        if let Some((src_canonical, src_name)) =
            ctx.resolve_named_type_export_target_shallow(canonical_id, decl_name)
        {
            if src_canonical.as_str() != canonical_id || src_name.as_str() != decl_name {
                return prepared_decl_body_is_closed(
                    ctx,
                    &src_canonical,
                    &src_name,
                    budget,
                    visited,
                );
            }
        }
        return false;
    };
    let no_bindings: KeyDomainBindings = KeyDomainBindings::default();
    key_domain_type_expr_is_closed(
        ctx,
        &prepared,
        &prepared.body,
        &no_bindings,
        OperandPosition::KeyDomain,
        budget,
        visited,
    )
}

/// Whether the instantiation `base<args…>` produces a surface whose
/// ENUMERABLE KEY SET can be PROVEN to close, given per-argument
/// identity-preserving bindings (`args[i]` = argument `i`'s
/// [`KeyDomainBinding`] — `Open`, or closed with the actual bound
/// `TypeExpr` / `SemanticNodeId` where scope-safely available, so the
/// conditional branch-selection oracle can select on a check that
/// references a parameter bound to a concrete argument).
///
/// Replaces the unsound "non-empty, all-concrete args ⇒ CLOSED" shortcut.
/// The instantiation's key domain closes only when:
///   1. the target declaration exists (a prepared decl is available),
///   2. its arity is satisfiable: `required_params <= arg_count <= total_params`
///      (the trailing `total_params - arg_count` params fall back to defaults),
///   3. every default supplied for an unfilled param is itself closed, and
///   4. the prepared body's KEY SET is closed under those bindings — a
///      type-parameter reference bound to a CLOSED arg/default counts as
///      closed; a type-parameter bound to an OPEN arg opens the body only
///      where it can influence the produced key set (the param appearing
///      as the body itself, in a keyof/mapped/indexed/conditional/heritage
///      position) — an open arg confined to member VALUE positions of a
///      fixed-key object body (`interface Foo<T> { label?: string;
///      items?: T }`) does NOT open the key domain, so `Omit<Foo<T>, …>`
///      still enumerates path-precisely. Any other open structure
///      (operator/conditional/mapped body over a free param, unresolved
///      `Ref`, instantiation over an open target) opens the instantiation.
///
/// Bounded by the same hop budget + a fresh visited set; prepared-decl cache
/// reads ONLY — no reducer, no substitution, no `execute_read`.
fn prepared_instantiation_key_domain_is_closed(
    ctx: &dyn crate::resolver_core::ResolverContext,
    base: &crate::semantic_query::DeclIdentity,
    args: &[KeyDomainBinding<'_>],
    budget: &mut u32,
) -> bool {
    let arg_count = args.len();
    // Root the consult of the target decl's file on the active fact tracer
    // — the key-domain verdict depends on the prepared body's content, so
    // an edit to it must reject the consuming warm entry.
    observe_closedness_walk_consult(ctx, base.canonical_id.as_ref());
    let prepared = match ctx.prepared_type_decl(base.canonical_id.as_ref(), base.decl_name.as_ref())
    {
        Some(prepared) => prepared,
        None => {
            // No local declaration at the base identity — it may be a barrel
            // RE-EXPORT (`export { SelectMenuProps } from './props'`) rather
            // than a declaration in this file. Follow ONE re-export hop
            // through the shallow export map (cache reads only) and retry on
            // the resolved source decl, budget-bounded against re-export
            // cycles. Without this an `Omit<SelectMenuProps<T>, K>` over a
            // barrel-reexported generic wrapper is mis-classified OPEN (the
            // wrapper's prepared decl lives in its source file, not the
            // barrel) and the L1 carrier-stop wrongly fires.
            if *budget == 0 {
                return false;
            }
            *budget -= 1;
            if let Some((src_canonical, src_name)) = ctx.resolve_named_type_export_target_shallow(
                base.canonical_id.as_ref(),
                base.decl_name.as_ref(),
            ) {
                if src_canonical.as_str() != base.canonical_id.as_ref()
                    || src_name.as_str() != base.decl_name.as_ref()
                {
                    let resolved = crate::semantic_query::DeclIdentity {
                        canonical_id: Arc::from(src_canonical.as_str()),
                        whole_hash: crate::semantic_query::HashValue::default(),
                        decl_name: Arc::from(src_name.as_str()),
                    };
                    return prepared_instantiation_key_domain_is_closed(
                        ctx, &resolved, args, budget,
                    );
                }
            }
            // Unresolved target ⇒ undecidable ⇒ not closed.
            return false;
        }
    };

    let total_params = prepared.type_parameters.len();
    let required_params = prepared
        .type_parameters
        .iter()
        .filter(|p| p.default.is_none())
        .count();
    // Over- or under-application: arity not satisfiable ⇒ not closed.
    if arg_count < required_params || arg_count > total_params {
        return false;
    }

    // Binding environment: every declared type-parameter name is bound —
    // the first `arg_count` to the caller's identity-preserving argument
    // bindings, the rest (unfilled, defaulted) to `ClosedAbstract`
    // pending the default verification below. Params bound to an OPEN
    // argument open the body only where the body places them in a
    // KEY-relevant position; unfilled params fall back to their
    // (verified-closed) defaults and stay closed.
    let mut bindings: KeyDomainBindings = KeyDomainBindings::default();
    for (param, binding) in prepared.type_parameters.iter().zip(args.iter()) {
        bindings.insert(param.name.as_str(), *binding);
    }
    for param in prepared.type_parameters.iter().skip(arg_count) {
        bindings.insert(param.name.as_str(), KeyDomainBinding::ClosedAbstract);
    }

    // Defaults for unfilled params must close UNDER THE SAME BINDINGS: a
    // default referencing an EARLIER type parameter (`<T, U = T>`) is
    // closed when that parameter is bound to a closed arg and open when
    // it is bound to an open one. A VERIFIED-CLOSED default then
    // RE-BINDS its parameter to the default's actual identity
    // (environment-free expr, forwarded earlier-param binding, or
    // own-scope-resolved named-ref node — `ClosedAbstract` only as the
    // no-identity fallback), so a conditional check over a defaulted
    // parameter (`<T, Use = true> … Use extends true ? …`) can select
    // through the shared oracle instead of deferring into an open
    // losing branch. Processed in declaration order so later defaults
    // see earlier defaults' identities.
    let mut chain_visited: FxHashSet<(Arc<str>, Arc<str>)> = FxHashSet::default();
    for param in prepared.type_parameters.iter().skip(arg_count) {
        let Some(default) = param.default.as_deref() else {
            // No default for an unfilled param — guarded by the arity check
            // above, but stay conservative.
            return false;
        };
        if !key_domain_type_expr_is_closed(
            ctx,
            &prepared,
            default,
            &bindings,
            OperandPosition::KeyDomain,
            budget,
            &mut chain_visited,
        ) {
            return false;
        }
        let default_binding = normalise_closed_arg_binding(ctx, &prepared, default, &bindings);
        bindings.insert(param.name.as_str(), default_binding);
    }

    key_domain_type_expr_is_closed(
        ctx,
        &prepared,
        &prepared.body,
        &bindings,
        OperandPosition::KeyDomain,
        budget,
        &mut chain_visited,
    )
}

/// Collect every `infer X` binding NAME reachable in `expr` into `sink`
/// (borrowing the names from the body tree). A conditional's `extends`
/// clause introduces these bindings for its branches; treating them as
/// bound leaves keeps an inferred-name reference (lowered as a bare `Ref`)
/// from being wrongly opened by the bare-unresolved-`Ref` rule. Shallow,
/// allocation-free typed-IR walk — no reduction.
fn collect_infer_names<'e>(expr: &'e TypeExpr, sink: &mut FxHashSet<&'e str>) {
    match expr {
        TypeExpr::Infer { name } => {
            sink.insert(name.as_str());
        }
        TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) | TypeExpr::KeyOf(inner) => {
            collect_infer_names(inner, sink)
        }
        TypeExpr::Array { element, .. } => collect_infer_names(element, sink),
        TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => {
            arms.iter().for_each(|a| collect_infer_names(a, sink))
        }
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .for_each(|e| collect_infer_names(&e.ty, sink)),
        TypeExpr::Ref { type_arguments, .. } => type_arguments
            .iter()
            .for_each(|a| collect_infer_names(a, sink)),
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            collect_infer_names(check, sink);
            collect_infer_names(extends, sink);
            collect_infer_names(true_type, sink);
            collect_infer_names(false_type, sink);
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_infer_names(object, sink);
            collect_infer_names(index, sink);
        }
        _ => {}
    }
}

/// THE single bound-param-aware TypeExpr KEY-DOMAIN classifier — shared
/// by every TypeExpr-layer closedness consult: bare prepared-decl bodies
/// ([`prepared_decl_body_is_closed`], EMPTY binding environment) and
/// instantiated bodies + unfilled-param defaults
/// ([`prepared_instantiation_key_domain_is_closed`], declared params
/// bound to identity-preserving [`KeyDomainBinding`]s). One classifier,
/// one set of arm semantics — the prepared-decl route and the
/// instantiated route cannot diverge on what closes a key domain.
///
/// Judges whether `body`'s enumerable KEY SET is closed under the binding
/// environment AT the given operand `position`
/// ([`OperandPosition::KeyDomain`] applies the per-argument key-domain
/// rule to instantiations; [`OperandPosition::ValueSensitive`] — entered
/// at `Conditional.check`/`Conditional.extends` and
/// `IndexedAccess.object` — judges an instantiation OPEN if ANY argument
/// is open, because the enclosing operator consumes the operand's
/// VALUES). A reference to a parameter bound to a verified-closed
/// arg/default/binder counts as closed; a reference to a parameter bound
/// OPEN is open — but only where the body actually references it: an
/// object body's member-name set is fixed regardless of its member
/// VALUES (the walk does not descend `Object` member values), so an open
/// param confined to value positions never opens the key domain.
/// Binder-introducing forms BIND their binder for the walk: a mapped
/// type binds its `[K in …]` parameter for the source/remap inspection,
/// a conditional binds its `infer` names for the branch inspection.
/// Conditionals are TRI-STATE through the shared branch-selection oracle
/// (see the `Conditional` arm). A free (unbound) type parameter, an
/// unresolved bare `Ref`, an instantiation over an open/unresolvable
/// target, or an unmodelled shape opens the body. Bounded by `budget`
/// (one decrement per inspected node); prepared-decl cache reads ONLY —
/// no reducer, no substitution, no `execute_read`; the one shared-engine
/// consult is the branch-selection oracle, which never materialises
/// branches. `visited` is the in-flight decl cycle guard threaded
/// through transparent alias hops.
#[allow(clippy::too_many_arguments)]
fn key_domain_type_expr_is_closed<'e>(
    ctx: &dyn crate::resolver_core::ResolverContext,
    prepared: &'e verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl,
    body: &'e TypeExpr,
    bindings: &KeyDomainBindings<'e>,
    position: OperandPosition,
    budget: &mut u32,
    visited: &mut FxHashSet<(Arc<str>, Arc<str>)>,
) -> bool {
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    match body {
        // Finite leaf surfaces: an enumerable key space / concrete value.
        TypeExpr::Primitive(_) | TypeExpr::Literal(_) => true,
        // An object body's NAMED members fix its key set regardless of
        // their value types — at `KeyDomain` the walk never descends
        // member values; at `ValueSensitive` the enclosing operator
        // consumes the object's VALUES, so member values (and
        // call/construct/method signatures) must close too, judged
        // value-sensitively. An index signature's KEY type IS key-domain
        // reachable in EVERY position. A concrete key (`[k: string]`) is
        // the bounded Record-class signature surface and does NOT
        // disqualify the domain (the materialiser publishes it as an
        // `IndexSignature` surface alongside the named members); a key
        // bound to an OPEN param, an unresolved name, or a template key
        // over an open interpolant leaves the produced key domain
        // undecidable ⇒ open.
        TypeExpr::Object(obj) => obj.properties.iter().all(|member| match member {
            verter_type_expr::ObjectMember::IndexSignature(sig) => {
                key_domain_type_expr_is_closed(
                    ctx,
                    prepared,
                    &sig.key_type,
                    bindings,
                    OperandPosition::KeyDomain,
                    budget,
                    visited,
                ) && (position == OperandPosition::KeyDomain
                    || key_domain_type_expr_is_closed(
                        ctx,
                        prepared,
                        &sig.value_type,
                        bindings,
                        OperandPosition::ValueSensitive,
                        budget,
                        visited,
                    ))
            }
            verter_type_expr::ObjectMember::Property(prop) => {
                position == OperandPosition::KeyDomain
                    || key_domain_type_expr_is_closed(
                        ctx,
                        prepared,
                        &prop.ty,
                        bindings,
                        OperandPosition::ValueSensitive,
                        budget,
                        visited,
                    )
            }
            verter_type_expr::ObjectMember::Method(method) => {
                position == OperandPosition::KeyDomain
                    || function_expr_value_is_closed(
                        ctx,
                        prepared,
                        &method.function,
                        bindings,
                        budget,
                        visited,
                    )
            }
            verter_type_expr::ObjectMember::CallSignature(function)
            | verter_type_expr::ObjectMember::ConstructSignature(function) => {
                position == OperandPosition::KeyDomain
                    || function_expr_value_is_closed(
                        ctx, prepared, function, bindings, budget, visited,
                    )
            }
        }),
        TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) => key_domain_type_expr_is_closed(
            ctx, prepared, inner, bindings, position, budget, visited,
        ),
        // Tuple/array ELEMENTS are VALUE positions: the KEY domain (the
        // indices / the Array-class signature surface) is fixed
        // regardless of element values — closed at `KeyDomain` without
        // descending, matching the node walk's leaf rule (tuples carve
        // out `rest` elements; see the `Tuple` arm); a `ValueSensitive`
        // operand consumes the element VALUES, so they must close.
        TypeExpr::Array { element, .. } => {
            position == OperandPosition::KeyDomain
                || key_domain_type_expr_is_closed(
                    ctx,
                    prepared,
                    element,
                    bindings,
                    OperandPosition::ValueSensitive,
                    budget,
                    visited,
                )
        }
        // Composites close iff every arm closes (a heritage `Intersection`,
        // a finite union source, a key-literal union argument).
        TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => arms.iter().all(|arm| {
            key_domain_type_expr_is_closed(ctx, prepared, arm, bindings, position, budget, visited)
        }),
        // A tuple's index KEY domain is fixed by its arity — EXCEPT a
        // `rest` element (`[string, ...T]`), whose arity contribution
        // depends on the rest TYPE: rest elements are judged at
        // `KeyDomain` in every position (an open rest element leaves the
        // index set undecidable ⇒ open), while non-rest elements stay
        // undescended closed leaves at `KeyDomain`. No tuple-arity
        // algebra: a rest element whose type closes at `KeyDomain`
        // (`...string[]`) conservatively keeps the domain closed.
        TypeExpr::Tuple { elements, .. } => {
            if position == OperandPosition::KeyDomain {
                elements.iter().filter(|e| e.rest).all(|e| {
                    key_domain_type_expr_is_closed(
                        ctx,
                        prepared,
                        &e.ty,
                        bindings,
                        OperandPosition::KeyDomain,
                        budget,
                        visited,
                    )
                })
            } else {
                elements.iter().all(|e| {
                    key_domain_type_expr_is_closed(
                        ctx,
                        prepared,
                        &e.ty,
                        bindings,
                        OperandPosition::ValueSensitive,
                        budget,
                        visited,
                    )
                })
            }
        }
        // A function/constructor TYPE at `KeyDomain` is not an
        // enumerable key surface (conservatively not-provably-closed,
        // matching the previous catch-all); under `ValueSensitive` the
        // operator consumes the function VALUE — every parameter and the
        // return type must close.
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => {
            position == OperandPosition::ValueSensitive
                && function_expr_value_is_closed(ctx, prepared, function, bindings, budget, visited)
        }
        // A first-class type-parameter reference is a LEAF type variable.
        // Bound to a verified-closed arg/default/binder it is closed (it
        // carries no own enumerable key space the outer filter would
        // expand); bound OPEN its key space IS the open arg's key space —
        // the body places the param in a key-reachable position, so the
        // domain opens. A FREE (unbound) parameter — e.g. inside a
        // bare-referenced generic decl body — is undecidable ⇒ open.
        TypeExpr::TypeParameter(param) => {
            matches!(bindings.get(param.name.as_str()), Some(binding) if !binding.is_open())
        }
        // A template-literal KEY closes iff every interpolant closes — a
        // bound binder interpolation (`` `on${K}` `` under
        // `[K in 'a' | 'b' as …]`) is a K-only transform over a finite
        // key space, CLOSED; an interpolant reaching an OPEN param keeps
        // the produced keys undecidable.
        TypeExpr::TemplateLiteral { expressions, .. } => expressions.iter().all(|e| {
            key_domain_type_expr_is_closed(ctx, prepared, e, bindings, position, budget, visited)
        }),
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            // A bare `Ref` (no type arguments) is a leaf reference. A
            // bound parameter / mapper binder / inference name is closed
            // unless bound OPEN; otherwise it is a transparent alias hop
            // — require the referenced declaration's key domain to
            // close. An unresolvable bare name is open (undecidable):
            // it is NOT an `infer`-introduced binding (those are bound by
            // the Conditional arm below) and treating it as a closed leaf
            // would wrongly prove an open body closed and let the L1
            // carrier-stop materialise an undecidable surface.
            if type_arguments.is_empty() {
                if let Some(binding) = bindings.get(name.as_ref()) {
                    return !binding.is_open();
                }
                return match prepared.name_resolution.get(name.as_ref()) {
                    Some(target) => prepared_decl_body_is_closed(
                        ctx,
                        &target.canonical_id,
                        &target.symbol_name,
                        budget,
                        visited,
                    ),
                    None => false,
                };
            }
            // A `Ref` WITH type arguments is a generic / builtin-utility
            // instantiation (a heritage arm `interface B extends
            // Omit<C, 'c1'> { … }`, or a generic wrapper body `type
            // Outer<T> = Foo<T>`). Each argument's own key-domain
            // openness is computed under the CURRENT bindings (a closed
            // argument keeps its identity — environment-free exprs and
            // forwarded parameter bindings — for the branch-selection
            // oracle downstream). How the instantiation itself is judged
            // depends on the POSITION:
            //
            // - `KeyDomain`: the per-argument rule — the target decl
            //   decides whether an open argument actually reaches a
            //   key-relevant position
            //   (`prepared_instantiation_key_domain_is_closed`). An open
            //   argument confined to member VALUE positions of a
            //   fixed-key target (`Foo<T>` over `interface Foo<T> {
            //   label?: string; items?: T }`) keeps the wrapper's key
            //   domain CLOSED — so `Omit<Outer<T>, 'items'>` over `type
            //   Outer<T> = Foo<T>` (or the `extends Foo<T>` heritage
            //   twin) still enumerates path-precisely. An UNSHADOWED
            //   builtin utility is judged by the ONE registry-owned rule
            //   (`builtin_utility_key_domain_is_closed`, shared with the
            //   node route).
            //
            // - `ValueSensitive`: the enclosing operator consumes this
            //   instantiation's VALUES (`Conditional.check`/`extends`,
            //   `IndexedAccess.object`) — ANY open argument opens it;
            //   with all arguments closed it is a concrete surface
            //   (resolvable target or registry builtin ⇒ closed,
            //   unresolvable ⇒ open).
            //
            // Bounded + cache-read-only (no reducer / no execute_read).
            let arg_bindings: Vec<KeyDomainBinding> = type_arguments
                .iter()
                .map(|a| {
                    if key_domain_type_expr_is_closed(
                        ctx, prepared, a, bindings, position, budget, visited,
                    ) {
                        normalise_closed_arg_binding(ctx, prepared, a, bindings)
                    } else {
                        KeyDomainBinding::Open
                    }
                })
                .collect();
            if position == OperandPosition::ValueSensitive {
                if arg_bindings.iter().any(|binding| binding.is_open()) {
                    return false;
                }
                return prepared.name_resolution.contains_key(name.as_ref())
                    || verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(
                        name.as_ref(),
                    )
                    .is_some();
            }
            match prepared.name_resolution.get(name.as_ref()) {
                Some(target) => {
                    let target_identity = crate::semantic_query::DeclIdentity {
                        canonical_id: Arc::from(target.canonical_id.as_str()),
                        whole_hash: crate::semantic_query::HashValue::default(),
                        decl_name: Arc::from(target.symbol_name.as_str()),
                    };
                    prepared_instantiation_key_domain_is_closed(
                        ctx,
                        &target_identity,
                        &arg_bindings,
                        budget,
                    )
                }
                None => builtin_utility_key_domain_is_closed(name.as_ref(), &arg_bindings),
            }
        }
        // TRI-STATE conditional closedness through the SHARED
        // branch-selection oracle (`type_expr_conditional_branch_selection`
        // → `ProjectSemanticDispatch::conditional_branch_selection` — the
        // same `shallow_relation_check` → `relate_nodes` path
        // `build_conditional` selects branches with):
        //
        //   True selected  ⇒ classify ONLY the true branch — an open
        //                    LOSING branch is dead and must not
        //                    false-OPEN the domain (`true extends true ?
        //                    { label: string } : T` is CLOSED).
        //   False selected ⇒ classify ONLY the false branch.
        //   Deferred       ⇒ classify the check/extends OPERANDS
        //                    value-sensitively (branch selection depends
        //                    on operand VALUES — any open instantiation
        //                    argument opens them) AND both branches under
        //                    the surrounding position.
        //
        // `infer X` bindings in the `check`/`extends` clauses bind `X`
        // for the conditional's branches (TS semantics) — harvested and
        // bound as `ClosedAbstract` leaves so a branch referencing an
        // inferred name (lowered as a bare `Ref`, e.g.
        // `ChatMessageProps<M, D, U>` where `M`/`D`/`U` are inferred in
        // `extends UIMessage<infer M, infer D, infer U>`) is NOT wrongly
        // opened by the bare-unresolved-`Ref` rule.
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            let mut branch_bindings = bindings.clone();
            let mut infer_names: FxHashSet<&str> = FxHashSet::default();
            collect_infer_names(extends, &mut infer_names);
            collect_infer_names(check, &mut infer_names);
            for &infer_name in &infer_names {
                branch_bindings
                    .entry(infer_name)
                    .or_insert(KeyDomainBinding::ClosedAbstract);
            }
            let (mut selection, infer) =
                type_expr_conditional_branch_selection(ctx, prepared, check, extends, bindings);
            match infer {
                Some(super::InferPatternSelection::BareInfer { name }) => {
                    // TRUE selected with `X := check` — bind the selected
                    // bare-infer name to the CHECK's own classification,
                    // mirroring the build-side substitution: `? X : …`
                    // over an open check stays honestly OPEN; a closed
                    // check forwards its identity into the branch.
                    if let Some(harvested) =
                        infer_names.iter().copied().find(|n| *n == name.as_ref())
                    {
                        let check_binding = if key_domain_type_expr_is_closed(
                            ctx,
                            prepared,
                            check,
                            bindings,
                            OperandPosition::ValueSensitive,
                            budget,
                            visited,
                        ) {
                            normalise_closed_arg_binding(ctx, prepared, check, bindings)
                        } else {
                            KeyDomainBinding::Open
                        };
                        branch_bindings.insert(harvested, check_binding);
                    }
                }
                Some(super::InferPatternSelection::FunctionInfer { .. }) => {
                    // A function-infer selection binds branch infer names
                    // to CHECK-SIGNATURE COMPONENTS this TypeExpr walk
                    // has no identity bindings for — classifying the raw
                    // branch with blindly-closed infer placeholders would
                    // risk a false-CLOSED. Widen to the Deferred
                    // treatment: a superset of the selected branch,
                    // conservative in the carrier direction.
                    selection = super::ConditionalBranchSelection::Deferred;
                }
                None => {}
            }
            match selection {
                super::ConditionalBranchSelection::True => key_domain_type_expr_is_closed(
                    ctx,
                    prepared,
                    true_type,
                    &branch_bindings,
                    position,
                    budget,
                    visited,
                ),
                super::ConditionalBranchSelection::False => key_domain_type_expr_is_closed(
                    ctx,
                    prepared,
                    false_type,
                    &branch_bindings,
                    position,
                    budget,
                    visited,
                ),
                super::ConditionalBranchSelection::Deferred => {
                    key_domain_type_expr_is_closed(
                        ctx,
                        prepared,
                        check,
                        bindings,
                        OperandPosition::ValueSensitive,
                        budget,
                        visited,
                    ) && key_domain_type_expr_is_closed(
                        ctx,
                        prepared,
                        extends,
                        bindings,
                        OperandPosition::ValueSensitive,
                        budget,
                        visited,
                    ) && key_domain_type_expr_is_closed(
                        ctx,
                        prepared,
                        true_type,
                        &branch_bindings,
                        position,
                        budget,
                        visited,
                    ) && key_domain_type_expr_is_closed(
                        ctx,
                        prepared,
                        false_type,
                        &branch_bindings,
                        position,
                        budget,
                        visited,
                    )
                }
            }
        }
        // The OBJECT operand is VALUE-sensitive: `Wrap<T>['a']` IS the
        // member VALUE `a` (e.g. `BigOpen<T>` — an open surface), so a
        // per-argument-closed-but-any-arg-open object must OPEN the
        // access. The INDEX stays a key/keyspace question.
        TypeExpr::IndexedAccess { object, index } => {
            key_domain_type_expr_is_closed(
                ctx,
                prepared,
                object,
                bindings,
                OperandPosition::ValueSensitive,
                budget,
                visited,
            ) && key_domain_type_expr_is_closed(
                ctx,
                prepared,
                index,
                bindings,
                OperandPosition::KeyDomain,
                budget,
                visited,
            )
        }
        TypeExpr::KeyOf(inner) => key_domain_type_expr_is_closed(
            ctx,
            prepared,
            inner,
            bindings,
            OperandPosition::KeyDomain,
            budget,
            visited,
        ),
        // A mapped body splits by ROLE. The produced key set derives
        // from the `in`-clause source and the `as`-clause remap ONLY —
        // both are KEY-PRODUCTION, walked PINNED at `KeyDomain`
        // regardless of the surrounding position. The member VALUE
        // expression can never change WHICH keys exist, so at
        // `KeyDomain` it is NOT inspected (`Omit<{ [K in 'a' | 'b']: T
        // }, 'a'>` has the fixed key set {a, b}; the open `T` publishes
        // shallowly in value position) — but a `ValueSensitive` operand
        // position consumes the mapped type's VALUES
        // (`{ [K in 'a']: T }['a']` IS the open `T`), so the value must
        // close there too, judged value-sensitively. The mapper binder
        // is BOUND for every recursion (`K` in
        // `` { [K in 'a' | 'b' as `on${K}`]: string } `` is a closed
        // local introduced by the mapper, not an unresolved free ref) —
        // mirroring the conditional `infer` bindings above and the
        // node-level walk's `scoped_with_bound_binder`.
        TypeExpr::Mapped {
            parameter,
            source,
            value,
            name_type,
            ..
        } => {
            let mut mapped_bindings = bindings.clone();
            mapped_bindings
                .entry(parameter.as_str())
                .or_insert(KeyDomainBinding::ClosedAbstract);
            key_domain_type_expr_is_closed(
                ctx,
                prepared,
                source,
                &mapped_bindings,
                OperandPosition::KeyDomain,
                budget,
                visited,
            ) && name_type.as_deref().is_none_or(|n| {
                key_domain_type_expr_is_closed(
                    ctx,
                    prepared,
                    n,
                    &mapped_bindings,
                    OperandPosition::KeyDomain,
                    budget,
                    visited,
                )
            }) && (position == OperandPosition::KeyDomain
                || key_domain_type_expr_is_closed(
                    ctx,
                    prepared,
                    value,
                    &mapped_bindings,
                    OperandPosition::ValueSensitive,
                    budget,
                    visited,
                ))
        }
        // `infer T` is a conditional-inference binding placeholder, not an
        // unbound generic — it does not leave an open key space.
        TypeExpr::Infer { .. } => true,
        // `typeof x`, recursive refs, function/constructor types, and any
        // shape not modelled above stay conservatively not-provably-closed.
        _ => false,
    }
}

/// VALUE-closedness of a function signature — the `ValueSensitive`
/// descent rule for [`key_domain_type_expr_is_closed`]'s
/// function-shaped surfaces (bare function/constructor types, object
/// method/call/construct members): every parameter type and the return
/// type must close, judged value-sensitively. The signature's OWN
/// generic parameters are bound locals (`ClosedAbstract` — they cannot
/// be the outer open generic), bound `or_insert` so an outer binding of
/// the same name keeps the conservative outer verdict.
fn function_expr_value_is_closed<'e>(
    ctx: &dyn crate::resolver_core::ResolverContext,
    prepared: &'e verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl,
    function: &'e verter_type_expr::FunctionExpr,
    bindings: &KeyDomainBindings<'e>,
    budget: &mut u32,
    visited: &mut FxHashSet<(Arc<str>, Arc<str>)>,
) -> bool {
    let mut fn_bindings = bindings.clone();
    for type_param in &function.type_parameters {
        fn_bindings
            .entry(type_param.name.as_str())
            .or_insert(KeyDomainBinding::ClosedAbstract);
    }
    function.parameters.iter().all(|param| {
        key_domain_type_expr_is_closed(
            ctx,
            prepared,
            &param.ty,
            &fn_bindings,
            OperandPosition::ValueSensitive,
            budget,
            visited,
        )
    }) && function.return_type.as_deref().is_none_or(|ret| {
        key_domain_type_expr_is_closed(
            ctx,
            prepared,
            ret,
            &fn_bindings,
            OperandPosition::ValueSensitive,
            budget,
            visited,
        )
    })
}

/// ONE registry-owned KEY-DOMAIN closedness rule for a builtin-utility
/// instantiation — shared VERBATIM by the node-level `__builtin__`
/// `InstantiationRef` arm of [`OpenWalk`] and the TypeExpr-layer
/// unresolved-`Ref` fallback of [`key_domain_type_expr_is_closed`], so
/// the same semantic shape cannot flip verdict by representation route.
///
/// The rule is PER-UTILITY OUTPUT-KEY semantics, owned by the
/// `BuiltinUtility` registry
/// ([`BuiltinUtility::key_domain_argument_positions`]): a utility's
/// produced key domain is closed iff every argument that actually
/// PRODUCES output keys is closed. The object filters (`Pick` / `Omit`)
/// judge the filtered source plus the key-selection argument; the
/// mapped utilities (`Partial` / `Required` / `Readonly`) judge the
/// source; `Record<K, V>` judges ONLY `K` — its value argument never
/// opens the produced key domain (`Omit<Record<'a', T>, 'x'>` stays
/// CLOSED and materialises through the filter). A VALUE-PRODUCING utility (`ReturnType`,
/// `InstanceType`, `Awaited`, `NonNullable`, the union/extraction and
/// string utilities, …) makes NO closed-key claim — its output derives
/// from argument VALUE structure the key-domain argument walk never
/// inspected, so a key-domain-closed argument vector must NOT prove the
/// produced key domain closed (`ReturnType<() => T>` stays a carrier).
/// An under-applied key-producing position is structurally undecidable
/// — not closed. A non-registry name is not a builtin — not closed by
/// this rule.
fn builtin_utility_key_domain_is_closed(decl_name: &str, args: &[KeyDomainBinding<'_>]) -> bool {
    use verter_semantic::analysis::type_solver::builtin::BuiltinUtility;
    let Some(utility) = BuiltinUtility::from_name(decl_name) else {
        return false;
    };
    let Some(positions) = utility.key_domain_argument_positions() else {
        return false;
    };
    positions
        .iter()
        .all(|&idx| args.get(idx).is_some_and(|binding| !binding.is_open()))
}

/// Node budget for the enumeration-domain openness walk
/// ([`enumeration_domain_node_is_open`]). The walk is a bounded, shallow
/// typed-IR inspection — it never reduces, substitutes, or loads new
/// files — so this cap only guards a pathological already-interned
/// graph; legitimate utility sources resolve in a handful of hops.
const ENUMERATION_DOMAIN_OPENNESS_NODE_BUDGET: u32 = 256;

/// Enumeration-domain argument index for an OBJECT-FILTER builtin utility
/// (`Pick` / `Omit`), if `base` names one.
///
/// The enumeration domain is the SOURCE type whose key space the filter
/// walks: argument 0 (`Pick<X, K>` / `Omit<X, K>` walk `X`'s keys).
///
/// Returns `None` for any instantiation that is NOT a `Pick` / `Omit`
/// object filter — including the mapped utilities (`Partial` / `Required`
/// / `Readonly`, guarded by their `MappedType` deferred shell + L3 budget)
/// and `Record` (an index-signature key domain, not finite enumeration).
/// Those keep their existing reduce / terminal behaviour and are not
/// subject to the open-domain carrier-stop.
fn enumeration_domain_arg_index(base: &crate::semantic_query::DeclIdentity) -> Option<usize> {
    if base.canonical_id.as_ref() != "__builtin__" {
        return None;
    }
    enumeration_domain_arg_index_for_name(base.decl_name.as_ref())
}

/// Whether an UNSHADOWED `decl_name` belongs to the L1 object-filter
/// utility family (`Pick` / `Omit`).
///
/// The SINGLE source of L1 family identity — derived from the shared
/// `BuiltinUtility` registry through
/// [`enumeration_domain_arg_index_for_name`], so the lowering entrance
/// (`lower.rs`) and the openness predicate can never diverge on family
/// membership (a one-place edit when the family ever changes). Callers
/// must have ALREADY established the name is unshadowed builtin scope
/// (shadowed names resolve as userland declarations).
pub(super) fn is_l1_object_filter_utility(decl_name: &str) -> bool {
    enumeration_domain_arg_index_for_name(decl_name).is_some()
}

/// Name-keyed core of [`enumeration_domain_arg_index`] — see that
/// function's family rationale.
fn enumeration_domain_arg_index_for_name(decl_name: &str) -> Option<usize> {
    use verter_semantic::analysis::type_solver::builtin::BuiltinUtility;
    // Utility identity is decided by the shared `BuiltinUtility` registry,
    // NOT a local name string match — a single source of truth for which
    // names are builtin utilities (the registry also owns arity / intrinsic
    // metadata).
    //
    // The L1 carrier-stop is scoped to the OBJECT-FILTER utilities `Pick` /
    // `Omit` ONLY. These two materialise their argument-0 source surface
    // (`object_filter_source_surface`) and so degenerate into full
    // cross-file generic expansion when that source is an open instantiation
    // (the `ChatMessages.vue` `Pick<PropsBase<T>, …>` hang). Their
    // enumeration domain is always argument 0 (`Pick<X, K>` / `Omit<X, K>`
    // walk `X`'s keys).
    //
    // `Partial` / `Required` / `Readonly` are NOT in the L1 family: they
    // lower to a `MappedType` (`build.rs`), which already fails closed —
    // an unavailable source/keyspace yields a deferred `Mapped` shell and
    // the L3 budget covers `MappedType` / `Instantiate` / `Conditional`. A
    // second L1 carrier-stop on them would be redundant.
    //
    // `Record` is NOT in the L1 family either: its domain is argument 0
    // (the KEYS), and `Record<string, V>` / `Record<number, V>` are
    // infinite key domains (index signatures), not a finite enumeration —
    // an open-domain carrier-stop on Record's key argument is a category
    // error. Record correctly falls back to a deferred mapped carrier; if
    // Record gains first-class support later, primitive key domains become
    // `IndexSignature` surfaces, not CLOSED finite enumeration.
    //
    // The union/extraction/function/promise/string utilities (`Extract`,
    // `Exclude`, `ReturnType`, `Awaited`, `Uppercase`, …) are likewise NOT
    // key-enumerating object filters and keep their existing reduce /
    // terminal behaviour.
    match BuiltinUtility::from_name(decl_name)? {
        BuiltinUtility::Pick | BuiltinUtility::Omit => Some(0),
        _ => None,
    }
}

/// Shallow-By-Default carrier-stop predicate (L1).
///
/// Under `Published(Navigate)`, an object-filter utility (`Pick` / `Omit`)
/// must NOT materialise an enumeration domain whose key space is OPEN or
/// undecidable — doing so degenerates into full cross-file generic
/// expansion of an unbound source (the `ChatMessages.vue`
/// `Pick<PropsBase<T>, …>` hang). Returns `true` when the utility's
/// enumeration domain (argument 0) is open or cannot be proven finite;
/// the reducer then keeps the `InstantiationRef` as a shallow carrier
/// (`Pick<…>` published verbatim). A CLOSED domain (a finite object
/// surface, a finite union/intersection of object surfaces, or a
/// concrete instantiation reached without crossing an open node) returns
/// `false`, so a legitimate `Pick<Foo, 'bar'>` /
/// `Pick<PropsBase<ConcreteMsg[]>, 'icon'>` still materialises only the
/// requested keys.
///
/// Pure typed-IR inspection — no reduction, no substitution, no string
/// matching. An unrecognised utility name returns `false` (not subject
/// to this carrier-stop), preserving userland operator-helper and
/// nominal-generic behaviour.
pub(super) fn utility_enumeration_domain_is_open_or_unknown(
    ctx: &dyn crate::resolver_core::ResolverContext,
    base: &crate::semantic_query::DeclIdentity,
    args: &[SemanticNodeId],
) -> bool {
    let Some(idx) = enumeration_domain_arg_index(base) else {
        return false;
    };
    let Some(&domain) = args.get(idx) else {
        // A recognised utility whose domain argument is absent is
        // structurally undecidable — treat the domain as open.
        return true;
    };
    OpenWalk::enumeration_domain().node_is_open(ctx, domain)
}

/// Shallow-By-Default carrier-stop predicate (L1), MAPPED-TYPE family.
///
/// A mapped type `{ [K in source]: value_expr }` (with optional `as`
/// name-remap) must NOT enumerate its key space and materialise the
/// per-key value under a publication demand when its produced surface
/// still depends on an unbound OUTER generic — doing so degenerates into
/// the per-key value loop over `node_modules` (the `ChatMessagesSlots<T>`
/// / `TableSlots<T>` storm). Returns `true` when ANY of the four mapped
/// inputs is open:
///
/// - the SOURCE or the KEYSPACE key domain (enumeration-domain policy —
///   a finite value surface does NOT open the KEY domain, so `Function` /
///   `Object` leaves stay closed there; only an unbound key space does);
/// - the `as`-clause NAME REMAP, evaluated under the value-body policy
///   (the remapped keys may depend on the outer generic);
/// - the VALUE BODY, evaluated under the value-body policy — the bound
///   mapper binder `K` is treated as CLOSED, finite value surfaces
///   (`Function` / `Object` / `Array` / `Tuple`) are DESCENDED so an
///   outer generic reached through a function parameter or object member
///   still opens the value, and a conditional value inspects BOTH
///   branches (a conditional whose selected branch reaches the outer
///   generic is open).
///
/// CLOSED (returns `false`) ⇒ a finite, outer-generic-free mapped type
/// (`Partial`/`Required`/`Readonly`, `{ [K in keyof Closed]: Closed[K] }`,
/// a K-only transform, a finite keyspace) which still enumerates
/// path-precisely. Pure typed-IR inspection — no reduction, no
/// substitution, no string matching.
pub(crate) fn mapped_type_is_open_or_unknown(
    ctx: &dyn crate::resolver_core::ResolverContext,
    source: SemanticNodeId,
    mapper: &crate::semantic_query::MapperKey,
) -> bool {
    if mapped_type_key_domain_is_open_or_unknown(ctx, source, mapper) {
        return true;
    }
    // VALUE BODY: does the produced value still depend on an unbound
    // OUTER generic? The bound mapper binder `K` is closed; an intrinsic
    // / helper over `K` (`Capitalize<K>`) or a resolution miss does NOT
    // open it (only an outer-generic argument does).
    OpenWalk::mapped_value_body(mapper.parameter_node).node_is_open(ctx, mapper.value_expr)
}

/// Lowering-entrance builtin argument openness: does `node` (a lowered
/// builtin type argument) still depend on unsubstituted generic
/// structure — an unbound `TypeParam` (including a mapper binder whose
/// per-key substitution happens later at a demand point), an open
/// operator carrier over one, an `Opaque` degradation, or an
/// undecidable walk?
///
/// The carrier-mode lowering gate (`Navigate` / `Skeleton` / `Shallow`)
/// consults this per argument: a builtin instantiation over an OPEN
/// argument must intern the `InstantiationRef` carrier instead of
/// executing eagerly — eager execution over carrier-shaped args bakes
/// `Opaque(Miss)` into the produced structure (a
/// `NonNullable<ChatSlots[K]>` conditional check with unbound `K`)
/// and destroys the deferred structure the demand points need for
/// per-key realization. Closed-argument builtins keep the eager
/// execute, byte-for-byte.
pub(crate) fn builtin_lowering_argument_is_open(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: SemanticNodeId,
) -> bool {
    OpenWalk::lowering_value_argument().node_is_open(ctx, node)
}

/// KEY-PRODUCTION axis of [`mapped_type_is_open_or_unknown`]: is the
/// mapped type's produced KEY SET open or undecidable, judging ONLY the
/// source / keyspace / `as`-remap (binder-bound KEY-DOMAIN policy) and
/// never the value body?
///
/// The empty-path Shallow surface enumerator gates per-key enumeration
/// on THIS axis alone: a mapped type with a CLOSED key domain and an
/// open VALUE body (`{ [K in keyof ChatSlots]?: … MB<T> … }`) still
/// enumerates its keys path-precisely — the per-key VALUES materialise
/// under `StructuralTransit(Navigate)` and keep the open generic as a
/// deferred carrier (shallow values). Value-body openness defers only
/// the operator MATERIALISATION routes (the full predicate above), not
/// the key enumeration.
pub(crate) fn mapped_type_key_domain_is_open_or_unknown(
    ctx: &dyn crate::resolver_core::ResolverContext,
    source: SemanticNodeId,
    mapper: &crate::semantic_query::MapperKey,
) -> bool {
    // KEY DOMAIN: does the produced KEY SPACE depend on an unbound OUTER
    // generic (making `keyof source` non-enumerable — the storm)? A mapped
    // source / key space with NO outer generic is always enumerable at
    // build time — a closed builtin-utility instantiation
    // (`Omit<MenuProps, …>`), a closed conditional, a finite literal
    // union — so it must NOT carrier-stop (`Partial`/`Required`/`Readonly`
    // over a closed utility source still enumerate). The key-domain walk
    // therefore asks the narrow "reaches outer generic" question, does
    // NOT descend the source's member VALUES (a member value reaching `T`
    // does not change WHICH keys exist: `{ [K in keyof {a: Foo<T>}]: V }`
    // still has the finite key `a`), and judges an instantiation in the
    // domain by the SAME per-argument key-domain closure rule as
    // `Pick`/`Omit` (`prepared_instantiation_key_domain_is_closed`): an
    // open argument confined to member VALUE positions of a fixed-key body
    // keeps the key domain CLOSED, so `{ [K in keyof Foo<T>]: string }`
    // over `interface Foo<T> { label?: string; items?: T }` still
    // enumerates `label` / `items`.
    if OpenWalk::mapped_key_domain(mapper.parameter_node).node_is_open(ctx, source) {
        return true;
    }
    if OpenWalk::mapped_key_domain(mapper.parameter_node).node_is_open(ctx, mapper.key_space) {
        return true;
    }
    // NAME REMAP: KEY-PRODUCTION, not a value body — the remap's result
    // IS the produced key set, so it is judged by the binder-bound
    // KEY-DOMAIN policy (the same per-argument rule as the source /
    // keyspace, matching the TypeExpr route's `name_type` arm):
    // `as keyof Foo<T>` over a fixed-key `Foo` (T value-confined) stays
    // CLOSED and enumerates; a direct outer-generic remap (`as T`) or a
    // value-sensitive conditional operand inside the remap stays OPEN.
    if let Some(remap) = mapper.name_remap {
        if OpenWalk::mapped_key_domain(mapper.parameter_node).node_is_open(ctx, remap) {
            return true;
        }
    }
    false
}

/// Walk policy distinguishing the two openness predicates that share the
/// bounded typed-IR open-structure walker
/// ([`OpenWalk::node_is_open`]).
///
/// The enumeration-domain walk (`Pick`/`Omit` source key space) and the
/// mapped value-body walk differ in three respects, captured here so the
/// recursive walker stays a SINGLE implementation:
///
/// - `bound_params`: `TypeParam` node ids treated as BOUND (closed) for
///   this walk. The mapped value-body walk binds the `[K in …]` mapper
///   parameter; the enumeration-domain walk binds nothing.
/// - `descend_value_surfaces`: whether finite value surfaces (`Object`,
///   `Function`, `Array`, `Tuple`) are DESCENDED to find an unbound
///   generic nested inside them. The enumeration-domain walk treats them
///   as closed leaves (a function-typed property does not open a KEY
///   domain); the mapped value-body walk descends (an outer `T` reached
///   through a function parameter / object member still opens the
///   produced value).
/// - `outer_generic_only`: the openness QUESTION the walk answers. The
///   enumeration-domain walk asks "is the KEY SPACE undecidable / not
///   provably finite" — so a resolution miss (`Opaque`), an open-bodied
///   alias chain (`DeclRef`), or a not-provably-closing instantiation all
///   count as open (conservative; the keys cannot be enumerated). The
///   mapped VALUE-body walk asks the NARROWER "does the value reach an
///   UNBOUND OUTER generic (a `TypeParam` ∉ `bound_params`)" — a
///   resolution miss, a `DeclRef` (declarations cannot reference the
///   mapper's outer generic), or an intrinsic / helper instantiation over
///   the BOUND binder `K` (`Capitalize<K>`, `MixedVis[K]`) does NOT
///   propagate the outer generic and is CLOSED; only a `TypeParam` ∉
///   `bound_params` reached directly OR through an instantiation /
///   operator ARGUMENT opens it. This keeps the carrier-stop from
///   over-firing on `Partial`/`Required`/`Readonly` / key-remap reducers
///   (which are decidable per-key once `K` is bound) while still
///   detecting the `ChatMessagesSlots<T>` / `TableSlots<T>` outer-generic
///   value bodies.
/// - `per_argument_key_domain`: how an `InstantiationRef` is judged. The
///   KEY-DOMAIN walks (`Pick`/`Omit` enumeration domain AND the mapped
///   source / key space) judge an instantiation by the per-argument
///   key-domain closure rule (`prepared_instantiation_key_domain_is_closed`
///   over the per-argument binding vector): an open argument confined to
///   member VALUE positions of a fixed-key body does NOT open the produced
///   key set. The mapped VALUE-body walk instead asks only "does an
///   ARGUMENT reach the outer generic" (any open argument opens) — the
///   target body is never consulted there.
/// - `position`: the CURRENT [`OperandPosition`]. Every walk starts at
///   `KeyDomain`; the `Conditional` arm enters its check/extends
///   operands and the `IndexedAccess` arm enters its object operand at
///   `ValueSensitive` (the enclosing operator consumes those operands'
///   VALUES — an instantiation there is OPEN if ANY argument is open,
///   regardless of `per_argument_key_domain`); conditional BRANCHES and
///   the indexed-access INDEX stay in the surrounding position.
///
/// `Conditional` nodes are TRI-STATE through the shared branch-selection
/// oracle ([`ProjectSemanticDispatch::conditional_branch_selection`] —
/// the same `shallow_relation_check` → `relate_nodes` path
/// `build_conditional` selects branches with): a SELECTED branch is the
/// conditional's surface and is classified ALONE (an open losing branch
/// is dead); a Deferred selection classifies the operands
/// value-sensitively plus BOTH branches.
///
/// Verdicts are MEMOIZED per `(node, position)` (`memo`): the
/// `InstantiationRef` per-argument collect is non-short-circuiting, and
/// the graph is hash-consed, so a repeated open node
/// (`Pick<Foo<T, T>, K>` — both args are the SAME `TypeParam` id) must
/// return its real verdict on revisit, not a "no new signal" `false`
/// that would corrupt the per-argument vector into a false-CLOSED; the
/// position key keeps a node reached both as a key-domain operand and as
/// a value-sensitive operand on two independent verdicts. Only an
/// IN-FLIGHT back-edge (`in_flight`) — a genuine cycle on the current
/// walk frontier — is closed-for-revisit, and that answer is never
/// memoized.
struct OpenWalk {
    bound_params: FxHashSet<SemanticNodeId>,
    /// Infer NAMES bound by an enclosing oracle-selected bare-infer
    /// conditional (`X := check` — see the tri-state `Conditional` arm):
    /// an `Infer` reference in the selected branch classifies as its
    /// bound node. Empty outside such a branch.
    bound_infers: FxHashMap<Arc<str>, SemanticNodeId>,
    descend_value_surfaces: bool,
    outer_generic_only: bool,
    per_argument_key_domain: bool,
    position: OperandPosition,
    budget: u32,
    in_flight: FxHashSet<(SemanticNodeId, OperandPosition)>,
    memo: FxHashMap<(SemanticNodeId, OperandPosition), bool>,
}

impl OpenWalk {
    /// Enumeration-domain policy (`Pick`/`Omit` source key space): finite
    /// value surfaces are closed leaves, no bound mapper binders, and the
    /// walk asks the "not-provably-finite key space" question
    /// (`outer_generic_only = false`). Conditionals are tri-state through
    /// the shared oracle (see [`OpenWalk`]).
    fn enumeration_domain() -> Self {
        Self {
            outer_generic_only: false,
            bound_params: FxHashSet::default(),
            bound_infers: FxHashMap::default(),
            descend_value_surfaces: false,
            per_argument_key_domain: true,
            position: OperandPosition::KeyDomain,
            budget: ENUMERATION_DOMAIN_OPENNESS_NODE_BUDGET,
            in_flight: FxHashSet::default(),
            memo: FxHashMap::default(),
        }
    }

    /// Mapped value-body policy: the bound mapper binder `K` is closed,
    /// finite value surfaces are descended, and the walk asks the NARROW
    /// "reaches an unbound outer generic" question (`outer_generic_only =
    /// true`; an instantiation is open iff an ARGUMENT is open —
    /// `per_argument_key_domain = false`). Conditionals are tri-state
    /// through the shared oracle. See [`OpenWalk`].
    fn mapped_value_body(bound_param: SemanticNodeId) -> Self {
        let mut bound_params = FxHashSet::default();
        bound_params.insert(bound_param);
        Self {
            bound_params,
            bound_infers: FxHashMap::default(),
            descend_value_surfaces: true,
            outer_generic_only: true,
            per_argument_key_domain: false,
            position: OperandPosition::KeyDomain,
            budget: ENUMERATION_DOMAIN_OPENNESS_NODE_BUDGET,
            in_flight: FxHashSet::default(),
            memo: FxHashMap::default(),
        }
    }

    /// Builtin lowering-argument policy: asks the NARROW "does this
    /// ARGUMENT still depend on unsubstituted generic structure"
    /// question for the lowering-entrance builtin carrier gate. No
    /// bound binders (every reachable `TypeParam` — including a mapper
    /// binder whose substitution happens later at a demand point — is
    /// unbound at lowering time), value surfaces are descended (the
    /// builtin consumes the argument's VALUES), and an instantiation is
    /// open iff an argument is open (`per_argument_key_domain =
    /// false`). Conditionals are tri-state through the shared oracle.
    fn lowering_value_argument() -> Self {
        Self {
            bound_params: FxHashSet::default(),
            bound_infers: FxHashMap::default(),
            descend_value_surfaces: true,
            outer_generic_only: true,
            per_argument_key_domain: false,
            position: OperandPosition::KeyDomain,
            budget: ENUMERATION_DOMAIN_OPENNESS_NODE_BUDGET,
            in_flight: FxHashSet::default(),
            memo: FxHashMap::default(),
        }
    }

    /// Mapped key-domain policy (mapped SOURCE / key space): asks the
    /// narrow "does the KEY SPACE reach an unbound outer generic"
    /// question (`outer_generic_only = true`) but does NOT descend value
    /// surfaces — a member value reaching the outer generic does not
    /// change which KEYS the source exposes, so descending it would
    /// wrongly carrier-stop `{ [K in keyof {a: Foo<T>}]: string }` (whose
    /// finite key set `{a}` is enumerable). An instantiation in the
    /// domain is judged by the SAME per-argument key-domain closure rule
    /// as `Pick`/`Omit` (`per_argument_key_domain = true`): an open
    /// argument confined to member VALUE positions of a fixed-key body
    /// keeps the produced key set CLOSED (`{ [K in keyof Foo<T>]: V }`
    /// with `T` value-position-only still enumerates). The bound binder
    /// `K` is closed. Conditionals are tri-state through the shared
    /// oracle. See [`OpenWalk`].
    fn mapped_key_domain(bound_param: SemanticNodeId) -> Self {
        let mut bound_params = FxHashSet::default();
        bound_params.insert(bound_param);
        Self {
            bound_params,
            bound_infers: FxHashMap::default(),
            descend_value_surfaces: false,
            outer_generic_only: true,
            per_argument_key_domain: true,
            position: OperandPosition::KeyDomain,
            budget: ENUMERATION_DOMAIN_OPENNESS_NODE_BUDGET,
            in_flight: FxHashSet::default(),
            memo: FxHashMap::default(),
        }
    }

    /// A child walk with the SAME policy + remaining budget and `binder`
    /// added to the bound set — used by the nested-`Mapped` arm so the
    /// nested mapper's own binder is BOUND while ITS source / key space /
    /// remap are inspected. The binder scope is local to that node, so the
    /// child gets fresh `memo`/`in_flight` state (verdicts computed under
    /// the extended bind set must not leak into the outer walk's memo, and
    /// vice versa); the caller copies the child's spent budget back so the
    /// walk stays globally bounded.
    fn scoped_with_bound_binder(&self, binder: SemanticNodeId) -> Self {
        let mut bound_params = self.bound_params.clone();
        bound_params.insert(binder);
        Self {
            bound_params,
            bound_infers: self.bound_infers.clone(),
            descend_value_surfaces: self.descend_value_surfaces,
            outer_generic_only: self.outer_generic_only,
            per_argument_key_domain: self.per_argument_key_domain,
            position: self.position,
            budget: self.budget,
            in_flight: FxHashSet::default(),
            memo: FxHashMap::default(),
        }
    }

    /// A child walk with the SAME policy + remaining budget and `name`
    /// bound to `node` in the infer-binding environment — used by the
    /// tri-state `Conditional` arm when the shared oracle selects TRUE
    /// via the bare-infer pattern (`X := check`): the selected branch's
    /// `Infer` references classify as the check node. The binding scope
    /// is local to that branch, so the child gets fresh
    /// `memo`/`in_flight` state (same rule as
    /// [`Self::scoped_with_bound_binder`]); the caller copies the
    /// child's spent budget back.
    fn scoped_with_bound_infer(&self, name: Arc<str>, node: SemanticNodeId) -> Self {
        let mut bound_infers = self.bound_infers.clone();
        bound_infers.insert(name, node);
        Self {
            bound_params: self.bound_params.clone(),
            bound_infers,
            descend_value_surfaces: self.descend_value_surfaces,
            outer_generic_only: self.outer_generic_only,
            per_argument_key_domain: self.per_argument_key_domain,
            position: self.position,
            budget: self.budget,
            in_flight: FxHashSet::default(),
            memo: FxHashMap::default(),
        }
    }

    /// Bounded typed-IR walk deciding whether `node` is OPEN — i.e. it
    /// still depends on unsubstituted generic structure or cannot be
    /// proven a finite surface under this walk's policy.
    ///
    /// OPEN ⇒ reaches an unsubstituted (non-bound) `TypeParam`, an
    /// `Opaque` (resolution miss / degraded), a conditional whose
    /// inspected operands are open, an `IndexedAccess`/`KeyOf`/`Mapped`
    /// over an open operand, an instantiation whose produced KEY SET
    /// depends on an open type argument (an open arg confined to member
    /// VALUE positions of a fixed-key body does not open the domain),
    /// an under-applied / arity-unsatisfied / open-bodied
    /// `InstantiationRef`, an unresolved or open-bodied `DeclRef` alias
    /// chain, or exhausts the walk budget. CLOSED ⇒ a finite surface
    /// proven without crossing an open node. `Infer` is a
    /// conditional-inference binding placeholder, NOT an unbound generic.
    fn node_is_open(
        &mut self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        node: SemanticNodeId,
    ) -> bool {
        let memo_key = (node, self.position);
        if let Some(&verdict) = self.memo.get(&memo_key) {
            // Completed verdict: return it VERBATIM. The hash-consed graph
            // shares one `SemanticNodeId` per structure, and the
            // `InstantiationRef` per-argument collect does not
            // short-circuit — a revisited OPEN node must stay `true`
            // (`Pick<Foo<T, T>, K>`: arg1 revisits arg0's `T`), or the
            // per-argument vector would prove a key-positioned open param
            // CLOSED and re-open the storm class behind the fuse. The
            // position key keeps key-domain and value-sensitive verdicts
            // for the same node independent.
            return verdict;
        }
        if !self.in_flight.insert(memo_key) {
            // IN-FLIGHT back-edge — a genuine cycle on the current walk
            // frontier. Closed for THIS revisit only (the cycle
            // contributes no new openness signal); never memoized, so a
            // later completed verdict for the node wins. Defensive: the
            // intern-only hash-consed semantic graph is a DAG today, so
            // this back-edge cannot fire at this layer.
            return false;
        }
        let verdict = self.node_openness_uncached(ctx, node);
        self.in_flight.remove(&memo_key);
        self.memo.insert(memo_key, verdict);
        verdict
    }

    /// Recurse into `node` AT the given operand position, restoring the
    /// surrounding position afterwards. Memo / in-flight keys carry the
    /// position, so a node reached both as a key-domain operand and as a
    /// value-sensitive operand keeps two independent verdicts.
    fn node_is_open_at(
        &mut self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        node: SemanticNodeId,
        position: OperandPosition,
    ) -> bool {
        let surrounding = self.position;
        self.position = position;
        let verdict = self.node_is_open(ctx, node);
        self.position = surrounding;
        verdict
    }

    /// Single-node verdict compute behind [`Self::node_is_open`]'s
    /// memoization. Never call directly — the memo/in-flight discipline
    /// lives in the wrapper.
    fn node_openness_uncached(
        &mut self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        node: SemanticNodeId,
    ) -> bool {
        if self.budget == 0 {
            // Walk budget exhausted. The enumeration-domain walk is
            // conservatively undecidable ⇒ open; the value-body walk
            // (`outer_generic_only`) asks "reaches an unbound outer
            // generic" — not finding one within budget answers that
            // question NO (closed), and the armed runaway fuse remains the
            // depth backstop. Over-firing the carrier-stop on a deep
            // CLOSED value would be the worse failure.
            return !self.outer_generic_only;
        }
        self.budget -= 1;

        let Some(data) = super::node_data_for(ctx, node) else {
            // Unresolved / un-interned node: the enumeration-domain walk
            // treats it as open (not provably finite); the value-body walk
            // treats it as closed (a missing node carries no outer
            // generic).
            return !self.outer_generic_only;
        };
        match data.as_ref() {
            // --- type parameter: open unless BOUND for this walk ---
            // An unsubstituted outer `TypeParam` is an unbound generic;
            // the bound mapper binder `K` (added to `bound_params` by the
            // value-body policy) is a closed local and does NOT open.
            SemanticNodeData::TypeParam { .. } => !self.bound_params.contains(&node),

            // A `DeclPlaceholder` is a RESOLVED-but-deferred declaration
            // reference (canonical + name) — semantically identical to a
            // `DeclRef` for openness, NOT a resolution miss. For the
            // enumeration-domain walk its closedness is decided by the
            // bounded prepared-decl transparent-alias-chain walk (a
            // not-provably-closed source key space is open). For the
            // value-body walk a declaration reference CANNOT carry the
            // mapper's outer generic, so it does NOT open the value.
            SemanticNodeData::Opaque(crate::semantic_query::QueryError::DeclPlaceholder {
                canonical_id,
                name,
                ..
            }) => {
                if self.outer_generic_only {
                    return false;
                }
                let mut chain_visited: FxHashSet<(Arc<str>, Arc<str>)> = FxHashSet::default();
                let mut hop_budget = KEY_DOMAIN_TYPE_EXPR_WALK_BUDGET;
                !prepared_decl_body_is_closed(
                    ctx,
                    canonical_id.as_ref(),
                    name.as_ref(),
                    &mut hop_budget,
                    &mut chain_visited,
                )
            }
            // A genuine resolution miss / degraded `Opaque` makes the KEY
            // space undecidable (enumeration-domain ⇒ open) but carries no
            // outer generic (value-body ⇒ closed).
            SemanticNodeData::Opaque(_) => !self.outer_generic_only,

            // `Infer` is a conditional-inference BINDING placeholder
            // (`extends UIMessage<infer M, …>`), NOT an unbound generic.
            // A name BOUND by an enclosing oracle-selected bare-infer
            // conditional (`X := check`) classifies as its bound node —
            // the same binding the build-side substitution applies; an
            // unbound placeholder stays closed.
            SemanticNodeData::Infer { name } => {
                match self.bound_infers.get(name.as_ref()).copied() {
                    Some(bound) => self.node_is_open(ctx, bound),
                    None => false,
                }
            }

            // --- operator shapes ---
            // TRI-STATE conditional through the SHARED branch-selection
            // oracle (the pre-relation infer-pattern cases, then the same
            // `shallow_relation_check` → `relate_nodes` path
            // `build_conditional` selects branches with; `any` / `error`
            // checks defer): a SELECTED branch IS the conditional's
            // surface — classify only it (an open losing branch is dead
            // and must not false-OPEN the domain), with a bare-infer
            // selection binding the branch's infer name to the CHECK
            // node (`X := check`, mirroring the build-side
            // substitution). A function-infer selection binds
            // check-SIGNATURE components — widened to the Deferred
            // treatment here (a superset of the selected branch;
            // classifying the raw branch with unbound-closed infer
            // placeholders would risk a false-CLOSED). A Deferred
            // selection classifies the check/extends OPERANDS
            // value-sensitively (branch selection depends on operand
            // VALUES — any open instantiation argument opens them) plus
            // BOTH branches under the surrounding position.
            SemanticNodeData::Conditional {
                check,
                extends,
                true_branch_ref,
                false_branch_ref,
                ..
            } => {
                let (check, extends) = (*check, *extends);
                let (true_branch, false_branch) = (*true_branch_ref, *false_branch_ref);
                let (mut selection, infer) =
                    ProjectSemanticDispatch::new(ctx).conditional_branch_selection(check, extends);
                let mut bare_infer_binding: Option<(Arc<str>, SemanticNodeId)> = None;
                match infer {
                    Some(super::InferPatternSelection::BareInfer { name }) => {
                        bare_infer_binding = Some((name, check));
                    }
                    Some(super::InferPatternSelection::FunctionInfer { .. }) => {
                        selection = super::ConditionalBranchSelection::Deferred;
                    }
                    None => {}
                }
                match selection {
                    super::ConditionalBranchSelection::True => match bare_infer_binding {
                        Some((name, bound)) => {
                            let mut scoped = self.scoped_with_bound_infer(name, bound);
                            let open = scoped.node_is_open(ctx, true_branch);
                            self.budget = scoped.budget;
                            open
                        }
                        None => self.node_is_open(ctx, true_branch),
                    },
                    super::ConditionalBranchSelection::False => {
                        self.node_is_open(ctx, false_branch)
                    }
                    super::ConditionalBranchSelection::Deferred => {
                        self.node_is_open_at(ctx, check, OperandPosition::ValueSensitive)
                            || self.node_is_open_at(ctx, extends, OperandPosition::ValueSensitive)
                            || self.node_is_open(ctx, true_branch)
                            || self.node_is_open(ctx, false_branch)
                    }
                }
            }
            // `keyof`'s value IS its base's KEY SET — the base re-enters
            // the `KeyDomain` position even under a value-sensitive
            // operand, matching the TypeExpr arm.
            SemanticNodeData::KeyOf { base } => {
                self.node_is_open_at(ctx, *base, OperandPosition::KeyDomain)
            }
            // An indexed access is open when EITHER the object OR the index
            // key space is open. The OBJECT operand is VALUE-sensitive:
            // `Wrap<T>['a']` IS the member VALUE `a` (e.g. `BigOpen<T>` —
            // an open surface), so a per-argument-closed-but-any-arg-open
            // object must OPEN the access. The INDEX stays a key/keyspace
            // question; a `TypeParam`-keyed access (`T[K]` with open `K`)
            // is just as undecidable as an open object.
            SemanticNodeData::IndexedAccess { object, index } => {
                self.node_is_open_at(ctx, *object, OperandPosition::ValueSensitive)
                    || self.index_key_is_open(ctx, index)
            }
            // A nested mapped type splits by ROLE. Its `source` / mapper
            // `key_space` / `as`-clause `name_remap` are KEY-PRODUCTION —
            // walked PINNED at `KeyDomain` regardless of the surrounding
            // position (a value-sensitive parent consumes the mapped
            // type's VALUES, but which KEYS the mapper produces is still
            // a key-domain question: `{ [K in Keys<T>]: string }['a']`
            // over a fixed-key `Keys<T>` must not false-OPEN). Its VALUE
            // body is consumed exactly when the surrounding policy
            // consumes values — a `ValueSensitive` operand position or a
            // value-body-descending walk — so
            // `{ [K in 'a']: T }['a']` IS the open `T` and opens. The
            // nested mapper's OWN binder is BOUND for every inspection
            // (the binder is bound in EVERY walk): a remap `` as
            // `on${K}` `` over a finite key space is a K-only transform,
            // not an open interpolant; a remap reaching an outer `T`
            // stays open. The binder scope is local to this node, hence
            // the scoped child walk (shared budget, fresh memo).
            SemanticNodeData::Mapped { source, mapper } => {
                let mut scoped = self.scoped_with_bound_binder(mapper.parameter_node);
                let mut open = scoped.node_is_open_at(ctx, *source, OperandPosition::KeyDomain)
                    || scoped.node_is_open_at(ctx, mapper.key_space, OperandPosition::KeyDomain)
                    || mapper.name_remap.is_some_and(|n| {
                        scoped.node_is_open_at(ctx, n, OperandPosition::KeyDomain)
                    });
                if !open
                    && (self.descend_value_surfaces
                        || self.position == OperandPosition::ValueSensitive)
                {
                    open = scoped.node_is_open(ctx, mapper.value_expr);
                }
                self.budget = scoped.budget;
                open
            }
            SemanticNodeData::TemplateLiteral { expressions, .. } => {
                expressions.iter().any(|e| self.node_is_open(ctx, *e))
            }
            // An instantiation's openness depends on the walk's question.
            // The KEY-DOMAIN walks (`Pick`/`Omit` enumeration domain AND
            // the mapped source / key space) judge per-argument: an open
            // type argument does NOT by itself open the produced KEY SET —
            // `Foo<T>` over a decl whose body has a FIXED set of member
            // NAMES (T confined to member VALUE positions) keeps a CLOSED
            // key domain, so `Omit<Foo<T>, …>` and
            // `{ [K in keyof Foo<T>]: V }` still enumerate path-precisely.
            // The per-argument openness vector feeds
            // `prepared_instantiation_key_domain_is_closed` (decl exists,
            // arity/defaults satisfiable, prepared body's KEY SET closed
            // under those bindings — an open-bound param opens it only
            // where the body places it in a key-reachable position).
            // The value-body walk asks only "does an ARGUMENT reach the
            // outer generic" — an intrinsic / helper instantiation over the
            // BOUND binder `K` (`Capitalize<K>`, `NonNullable<X[K]>`) does
            // NOT propagate the outer generic and is CLOSED regardless of
            // whether its target body is provably closed; only
            // `Foo<T>` / `Base<T>` (an outer-generic argument) opens it.
            SemanticNodeData::InstantiationRef { base, args } => {
                // Non-short-circuiting per-argument collect — the memo in
                // `node_is_open` keeps a hash-consed repeated open node
                // truthful on revisit. Closed arguments keep their NODE
                // identity (the binding the conditional branch-selection
                // oracle resolves bound-parameter operands through).
                let arg_bindings: Vec<KeyDomainBinding> = args
                    .iter()
                    .map(|a| {
                        if self.node_is_open(ctx, *a) {
                            KeyDomainBinding::Open
                        } else {
                            KeyDomainBinding::ClosedNode(*a)
                        }
                    })
                    .collect();
                let any_open = arg_bindings.iter().any(|binding| binding.is_open());
                if self.position == OperandPosition::ValueSensitive {
                    // A VALUE-SENSITIVE operand position (conditional
                    // check/extends, indexed-access object): the
                    // enclosing operator consumes this instantiation's
                    // VALUES, so the per-argument key-domain rule does
                    // not apply — ANY open argument opens it.
                    if any_open {
                        return true;
                    }
                    if self.outer_generic_only {
                        // The outer-generic-only walks ask only "does an
                        // argument reach the outer generic" — an
                        // unresolvable base carries none (the DeclRef
                        // rule), so all-closed args stay closed.
                        return false;
                    }
                    // Enumeration-domain walk: all-closed arguments are a
                    // CONCRETE surface only when the base actually
                    // resolves (prepared decl / registry builtin) —
                    // mirroring the TypeExpr arm's
                    // `name_resolution`/registry gate; an unresolvable
                    // base is undecidable ⇒ open.
                    return !instantiation_base_is_resolvable(ctx, base, &mut self.budget);
                }
                if !self.per_argument_key_domain {
                    // The value-body walk asks only "does an ARGUMENT
                    // reach the outer generic".
                    return any_open;
                }
                if self.outer_generic_only && !any_open {
                    // Mapped KEY-DOMAIN policy with NO outer generic
                    // reaching the instantiation: the produced key set is
                    // concrete at build time — enumeration (or the
                    // deferred mapped shell on plain unavailability) owns
                    // it; no carrier-stop. Only the enumeration-domain
                    // walk (`outer_generic_only = false`) must also PROVE
                    // finiteness for concrete instantiations, because the
                    // `Pick`/`Omit` reducer would otherwise materialise an
                    // undecidable source.
                    return false;
                }
                if base.canonical_id.as_ref() == "__builtin__" {
                    // A `__builtin__` base has NO prepared decl, so the
                    // prepared-decl key-domain check below could never
                    // prove it closed. Judged by the ONE registry-owned
                    // rule (`builtin_utility_key_domain_is_closed`,
                    // shared verbatim with the TypeExpr route so the
                    // verdict is route-independent): per-utility
                    // OUTPUT-KEY semantics — only the arguments that
                    // actually produce output keys are judged
                    // (`Record`'s open value arg keeps the domain
                    // CLOSED; a value-producing utility makes no
                    // closed-key claim) — and a nested closed carrier
                    // (`Pick<Pick<{…}, 'a' | 'b'>, 'a'>` or
                    // `Pick<Partial<{…}>, 'a'>`) must NOT be judged
                    // OPEN.
                    return !builtin_utility_key_domain_is_closed(
                        base.decl_name.as_ref(),
                        &arg_bindings,
                    );
                }
                !prepared_instantiation_key_domain_is_closed(
                    ctx,
                    base,
                    &arg_bindings,
                    &mut self.budget,
                )
            }

            // --- carriers we can follow one transparent hop ---
            SemanticNodeData::Alias(target) => self.node_is_open(ctx, *target),
            SemanticNodeData::DeclRef { identity } => {
                // For the value-body walk a declaration reference cannot
                // carry the mapper's outer generic (declarations have their
                // own type parameters), so a bare `DeclRef` is CLOSED — an
                // outer generic reaches the value only as an instantiation
                // ARGUMENT (`Foo<T>`), handled by `InstantiationRef`. For
                // the enumeration-domain walk a resolved declaration whose
                // BOUNDED prepared-decl walk proves a finite key domain
                // (`key_domain_type_expr_is_closed` under an empty binding
                // environment) is CLOSED; anything else (a free type
                // parameter, an unresolved name, a cycle, or budget
                // exhaustion) is open.
                if self.outer_generic_only {
                    return false;
                }
                let mut chain_visited: FxHashSet<(Arc<str>, Arc<str>)> = FxHashSet::default();
                let mut hop_budget = KEY_DOMAIN_TYPE_EXPR_WALK_BUDGET;
                !prepared_decl_body_is_closed(
                    ctx,
                    identity.canonical_id.as_ref(),
                    identity.decl_name.as_ref(),
                    &mut hop_budget,
                    &mut chain_visited,
                )
            }

            // --- finite value surfaces ---
            // Under the key-domain policies these are closed leaves
            // (their NAMED members' presence makes the KEY space
            // enumerable) — EXCEPT an index signature's KEY type, which is
            // key-domain reachable in every policy: a key over an unbound
            // generic / undecidable structure leaves the produced key set
            // open even though the named-member set is fixed, while a
            // concrete key (`[k: string]`) is the bounded Record-class
            // signature surface and stays closed. The mapped value-body
            // policy — and ANY walk standing at a `ValueSensitive`
            // operand (the enclosing operator consumes these surfaces'
            // VALUES) — DESCENDS the value surfaces to find an unbound
            // generic nested in a member value / function parameter /
            // element type.
            SemanticNodeData::Object(view) => {
                if view
                    .index_signatures
                    .iter()
                    .any(|s| self.node_is_open(ctx, s.key_type))
                {
                    return true;
                }
                (self.descend_value_surfaces || self.position == OperandPosition::ValueSensitive)
                    && (view.members.iter().any(|m| self.node_is_open(ctx, m.value))
                        || view
                            .call_signatures
                            .iter()
                            .any(|s| self.node_is_open(ctx, *s))
                        || view
                            .construct_signatures
                            .iter()
                            .any(|s| self.node_is_open(ctx, *s))
                        || view
                            .index_signatures
                            .iter()
                            .any(|s| self.node_is_open(ctx, s.value_type)))
            }
            SemanticNodeData::Function {
                params,
                return_type,
                ..
            } => {
                (self.descend_value_surfaces || self.position == OperandPosition::ValueSensitive)
                    && (params.iter().any(|p| self.node_is_open(ctx, p.ty))
                        || self.node_is_open(ctx, *return_type))
            }
            SemanticNodeData::Array { element, .. } => {
                (self.descend_value_surfaces || self.position == OperandPosition::ValueSensitive)
                    && self.node_is_open(ctx, *element)
            }
            // A tuple's index KEY domain is fixed by its arity — EXCEPT
            // a `rest` element (`[string, ...T]`), whose arity
            // contribution depends on the rest TYPE: rest elements are
            // judged at `KeyDomain` in every position (an open rest
            // element leaves the index set undecidable ⇒ open), while
            // non-rest elements stay undescended closed leaves at
            // `KeyDomain`. No tuple-arity algebra: a rest element whose
            // type closes at `KeyDomain` conservatively keeps the
            // domain closed. Matches the TypeExpr `Tuple` arm.
            SemanticNodeData::Tuple { elements, .. } => {
                if elements.iter().any(|e| {
                    e.rest && self.node_is_open_at(ctx, e.value, OperandPosition::KeyDomain)
                }) {
                    return true;
                }
                (self.descend_value_surfaces || self.position == OperandPosition::ValueSensitive)
                    && elements.iter().any(|e| self.node_is_open(ctx, e.value))
            }
            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::VueMacroElements(_) => false,

            // --- composites: open iff any arm is open ---
            SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
                arms.iter().any(|a| self.node_is_open(ctx, *a))
            }
            SemanticNodeData::MergedDecl { contributors } => {
                contributors.iter().any(|a| self.node_is_open(ctx, *a))
            }

            // --- unresolved / fidelity carriers ---
            // A constructor type delegates to its inner function signature
            // (which carries the value-surface descent rule).
            SemanticNodeData::ConstructorType { signature } => self.node_is_open(ctx, *signature),
            // The three unresolved carriers that apply type arguments
            // (`typeof make<T>`, `import("m").Box<T>`, `Foo<T>`): OPEN if any
            // applied type argument reaches the outer generic — scanned through
            // the shared carrier-arg accessor so the value-body walk does NOT
            // false-close `Foo<T>` / `typeof make<T>` over an open `T`. With NO
            // open argument the carrier itself holds no outer generic (a
            // value-rooted `typeof` lookup / an unresolved name / a dynamic
            // import carries none), so it stays undecidable (open) for the
            // enumeration-domain walk and closed for the value-body walk — the
            // existing undecidable-root rule, now applied AFTER the type-arg
            // openness check.
            SemanticNodeData::TypeOf(_)
            | SemanticNodeData::ImportType(_)
            | SemanticNodeData::BareRef(_) => {
                data.carrier_type_args()
                    .iter()
                    .any(|a| self.node_is_open(ctx, *a))
                    || !self.outer_generic_only
            }
            // An unresolved raw-fallback carrier holds no type arguments and no
            // outer generic (closed for the value-body walk) but is undecidable
            // for the enumeration-domain walk.
            SemanticNodeData::RawFallback { .. } => !self.outer_generic_only,
            // A synthetic slot-binding is a concrete shallow terminal.
            SemanticNodeData::SyntheticBinding { .. } => false,
        }
    }

    /// Whether an [`IndexKey`](crate::semantic_query::IndexKey) index
    /// operand is OPEN. Literal string / number keys are concrete
    /// (closed); a `TypeNode` key is open iff its referenced node is open
    /// under the same bounded walk — always AT the `KeyDomain` position
    /// (the index is a key/keyspace question even when the enclosing
    /// access sits in a value-sensitive operand).
    fn index_key_is_open(
        &mut self,
        ctx: &dyn crate::resolver_core::ResolverContext,
        index: &crate::semantic_query::IndexKey,
    ) -> bool {
        use crate::semantic_query::IndexKey;
        match index {
            IndexKey::String(_) | IndexKey::Number(_) => false,
            IndexKey::TypeNode(node) => {
                self.node_is_open_at(ctx, *node, OperandPosition::KeyDomain)
            }
        }
    }
}

/// Whether an `InstantiationRef` BASE resolves to a known declaration —
/// the node-route mirror of the TypeExpr arm's
/// `name_resolution`/registry gate, consulted by the enumeration-domain
/// walk at a `ValueSensitive` operand with all-closed arguments: an
/// unresolvable base is an undecidable surface, not a concrete one.
/// `__builtin__` bases resolve through the `BuiltinUtility` registry;
/// file bases through the prepared-decl cache, with ONE barrel
/// re-export hop (deeper re-export chains stay conservatively
/// unresolvable ⇒ OPEN, the safe direction). The consult is rooted on
/// the active fact tracer — the verdict depends on the consulted files'
/// content.
fn instantiation_base_is_resolvable(
    ctx: &dyn crate::resolver_core::ResolverContext,
    base: &crate::semantic_query::DeclIdentity,
    budget: &mut u32,
) -> bool {
    if base.canonical_id.as_ref() == "__builtin__" {
        return verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(
            base.decl_name.as_ref(),
        )
        .is_some();
    }
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    observe_closedness_walk_consult(ctx, base.canonical_id.as_ref());
    if ctx
        .prepared_type_decl(base.canonical_id.as_ref(), base.decl_name.as_ref())
        .is_some()
    {
        return true;
    }
    if let Some((src_canonical, src_name)) = ctx.resolve_named_type_export_target_shallow(
        base.canonical_id.as_ref(),
        base.decl_name.as_ref(),
    ) {
        if src_canonical.as_str() != base.canonical_id.as_ref()
            || src_name.as_str() != base.decl_name.as_ref()
        {
            observe_closedness_walk_consult(ctx, src_canonical.as_str());
            return ctx
                .prepared_type_decl(src_canonical.as_str(), src_name.as_str())
                .is_some();
        }
    }
    false
}

fn semantic_primitive_to_type_expr(kind: SemanticPrimitiveKind) -> TypeExpr {
    use verter_type_expr::PrimitiveName;

    TypeExpr::Primitive(match kind {
        SemanticPrimitiveKind::String => PrimitiveName::String,
        SemanticPrimitiveKind::Number => PrimitiveName::Number,
        SemanticPrimitiveKind::Boolean => PrimitiveName::Boolean,
        SemanticPrimitiveKind::Symbol => PrimitiveName::Symbol,
        SemanticPrimitiveKind::BigInt => PrimitiveName::BigInt,
        SemanticPrimitiveKind::Any => PrimitiveName::Any,
        SemanticPrimitiveKind::Unknown => PrimitiveName::Unknown,
        SemanticPrimitiveKind::Void => PrimitiveName::Void,
        SemanticPrimitiveKind::Never => PrimitiveName::Never,
        SemanticPrimitiveKind::Null => PrimitiveName::Null,
        SemanticPrimitiveKind::Undefined => PrimitiveName::Undefined,
        SemanticPrimitiveKind::Object => PrimitiveName::Object,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::semantic_query::{
        IndexKey, PrimitiveKind as SemanticPrimitiveKind, SemanticNodeData,
    };
    use crate::VerterHost;
    use verter_type_expr::TypeExpr;

    use super::ProjectSemanticDispatch;

    #[test]
    fn raise_node_to_type_expr_preserves_number_index_key_values() {
        let host = VerterHost::new_standalone(Default::default());
        let graph = Arc::clone(host.project_type_store().semantic_graph());
        let object = graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::Unknown));
        let indexed = graph.intern_node(SemanticNodeData::IndexedAccess {
            object,
            index: IndexKey::Number(
                crate::semantic_query::CanonicalIndexInt::from_canonical_i64(7).expect("canonical"),
            ),
        });

        let dispatch = ProjectSemanticDispatch::new(&host);
        let expr = dispatch
            .raise_node_to_type_expr(indexed)
            .expect("indexed-access semantic node should serialize");

        let TypeExpr::IndexedAccess { index, .. } = &expr else {
            panic!("expected IndexedAccess expr, got {expr:?}");
        };
        assert_eq!(
            **index,
            TypeExpr::number_literal(7.0),
            "numeric index keys should serialize as number literals",
        );
    }

    /// An intersection whose EVERY arm is vacuous (`{} & {}`) must fall
    /// back to the representable empty object `{}` — never publish a
    /// zero-arm `TypeExpr::Intersection([])`.
    #[test]
    fn raise_all_vacuous_intersection_falls_back_to_empty_object() {
        let host = VerterHost::new_standalone(Default::default());
        let graph = Arc::clone(host.project_type_store().semantic_graph());
        let empty_a = graph.intern_node(SemanticNodeData::Object(
            crate::project_semantic_dispatch::walk::empty_surface_view(),
        ));
        let intersection = graph.intern_node(SemanticNodeData::Intersection(Arc::from(
            vec![empty_a, empty_a].into_boxed_slice(),
        )));

        let dispatch = ProjectSemanticDispatch::new(&host);
        let expr = dispatch
            .raise_node_to_type_expr(intersection)
            .expect("intersection must raise");

        match &expr {
            TypeExpr::Object(object) => {
                assert!(
                    object.properties.is_empty(),
                    "all-vacuous intersection must raise as the EMPTY object"
                );
            }
            TypeExpr::Intersection(arms) => {
                panic!("zero/filtered-arm Intersection must not publish (got {arms:?})")
            }
            other => panic!("expected empty Object, got {other:?}"),
        }
    }

    #[test]
    fn raise_node_to_type_expr_round_trips_primitive() {
        let host = VerterHost::new_standalone(Default::default());
        let graph = Arc::clone(host.project_type_store().semantic_graph());
        let node = graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::String));

        let dispatch = ProjectSemanticDispatch::new(&host);
        let expr = dispatch
            .raise_node_to_type_expr(node)
            .expect("primitive must raise");

        assert!(
            matches!(expr, TypeExpr::Primitive(_)),
            "primitive should round-trip, got {expr:?}"
        );
    }

    /// FAIL-FIRST: preserves a deferred operator over a free
    /// `TypeParameter`. `KeyOf(TypeParameter)` survives `raise_and_reduce`
    /// because dispatch returns the deferred operator over the free
    /// parameter unchanged (deferred-form policy); a reducer that eagerly
    /// collapsed it would drop the operator and FAIL this test.
    #[test]
    fn raise_and_reduce_preserves_open_keyof_over_type_parameter() {
        use crate::semantic_query::{DeclIdentity, HashValue, ProjectionMode};

        let host = VerterHost::new_standalone(Default::default());
        let graph = Arc::clone(host.project_type_store().semantic_graph());
        let identity = DeclIdentity {
            canonical_id: Arc::from("/test.ts"),
            whole_hash: HashValue::default(),
            decl_name: Arc::from("T"),
        };
        let type_param = graph.intern_node(SemanticNodeData::TypeParam {
            decl: identity,
            param_index: 0,
            constraint: None,
            default: None,
            display_name: Arc::from("T"),
        });
        let keyof = graph.intern_node(SemanticNodeData::KeyOf { base: type_param });

        let dispatch = ProjectSemanticDispatch::new(&host);
        let materialized = dispatch.raise_and_reduce(keyof, ProjectionMode::Expanded);

        assert!(
            matches!(materialized.type_expr, TypeExpr::KeyOf(_)),
            "open keyof over type parameter must survive raise_and_reduce, got {:?}",
            materialized.type_expr
        );
    }

    /// FAIL-FIRST: the iterative reducer terminates
    /// even when the visited set is the only termination signal. The visited
    /// set short-circuits the cycle and returns the alias body; a reducer
    /// without that guard would loop on the cycle and FAIL to terminate.
    #[test]
    fn raise_and_reduce_terminates_on_alias_cycle_via_visited_set() {
        use crate::semantic_query::ProjectionMode;

        let host = VerterHost::new_standalone(Default::default());
        let graph = Arc::clone(host.project_type_store().semantic_graph());
        let primitive =
            graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::String));
        let alias = graph.intern_node(SemanticNodeData::Alias(primitive));

        let dispatch = ProjectSemanticDispatch::new(&host);
        let materialized = dispatch.raise_and_reduce(alias, ProjectionMode::Expanded);

        assert!(
            matches!(materialized.type_expr, TypeExpr::Primitive(_)),
            "alias to primitive must reduce to that primitive, got {:?}",
            materialized.type_expr
        );
    }

    /// FAIL-FIRST: hard-stop for `TemplateLiteral` —
    /// no dispatch variant exists, so the reducer must convert to
    /// `Unknown { raw: "<unresolved template literal type>" }`.
    #[test]
    fn raise_and_reduce_template_literal_becomes_unknown_hard_stop() {
        use crate::semantic_query::ProjectionMode;

        let host = VerterHost::new_standalone(Default::default());
        let graph = Arc::clone(host.project_type_store().semantic_graph());
        let template = graph.intern_node(SemanticNodeData::TemplateLiteral {
            quasis: Arc::from(vec![Arc::from("prefix-")].into_boxed_slice()),
            expressions: Arc::from(
                Vec::<crate::semantic_query::SemanticNodeId>::new().into_boxed_slice(),
            ),
        });

        let dispatch = ProjectSemanticDispatch::new(&host);
        let materialized = dispatch.raise_and_reduce(template, ProjectionMode::Expanded);

        match &materialized.type_expr {
            TypeExpr::Unknown { raw } => {
                assert!(
                    raw.contains("template literal"),
                    "template literal hard-stop should mention the operator, got {raw:?}"
                );
            }
            other => panic!("expected Unknown hard-stop, got {other:?}"),
        }
    }

    /// FAIL-FIRST: Navigate-mode keeps a `DeclRef`
    /// terminal — a freshly-interned `DeclRef` raises to a bare
    /// `Ref { name }` with empty type arguments; a Navigate-mode reducer
    /// that eagerly expanded the carrier would lose the terminal `Ref` and
    /// FAIL this test.
    #[test]
    fn raise_and_reduce_navigate_mode_decl_ref_raises_to_bare_ref() {
        use crate::semantic_query::{DeclIdentity, HashValue, ProjectionMode};

        let host = VerterHost::new_standalone(Default::default());
        let graph = Arc::clone(host.project_type_store().semantic_graph());
        let identity = DeclIdentity {
            canonical_id: Arc::from("/some-unresolved.ts"),
            whole_hash: HashValue::default(),
            decl_name: Arc::from("Unresolved"),
        };
        let decl_ref = graph.intern_node(SemanticNodeData::DeclRef {
            identity: identity.clone(),
        });

        let dispatch = ProjectSemanticDispatch::new(&host);
        let materialized = dispatch.raise_and_reduce(decl_ref, ProjectionMode::Navigate);

        match &materialized.type_expr {
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                assert_eq!(name.as_ref(), "Unresolved");
                assert!(
                    type_arguments.is_empty(),
                    "navigate-mode DeclRef must raise without type arguments"
                );
            }
            // DeclRef in Navigate dispatches ResolveDecl → if dispatch
            // produces an Opaque(Miss) (no real prepared decl), the
            // reducer accepts it and the raise yields Unknown. Both
            // outcomes prove the lazy carrier was visited; the test
            // discriminates on the absence of `graphNode` text.
            TypeExpr::Unknown { raw } => {
                assert!(
                    !raw.starts_with("graphNode"),
                    "raise must not emit graphNode placeholder, got {raw:?}"
                );
            }
            other => panic!("expected Ref{{name=Unresolved}} or Unknown, got {other:?}"),
        }
    }

    #[test]
    fn raise_node_to_type_expr_round_trips_indexed_access_string_key() {
        // Discriminator: IndexedAccess with a String index key must
        // raise to TypeExpr::IndexedAccess { index: TypeExpr::Literal(...) }
        // — proves the helper `index_key_to_type_expr` follows the same
        // structural conversion as numeric keys without introducing the
        // `_inner` recursive call (cycle invariant for strings: there is
        // no node to recurse into).
        let host = VerterHost::new_standalone(Default::default());
        let graph = Arc::clone(host.project_type_store().semantic_graph());
        let object = graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::Unknown));
        let indexed = graph.intern_node(SemanticNodeData::IndexedAccess {
            object,
            index: IndexKey::String(Arc::from("key")),
        });

        let dispatch = ProjectSemanticDispatch::new(&host);
        let expr = dispatch
            .raise_node_to_type_expr(indexed)
            .expect("indexed-access semantic node should serialize");

        let TypeExpr::IndexedAccess { index, .. } = &expr else {
            panic!("expected IndexedAccess expr, got {expr:?}");
        };
        assert_eq!(
            **index,
            TypeExpr::string_literal("key"),
            "string index keys should serialize as string literals",
        );
    }

    /// L1 carrier-stop predicate (Shallow-By-Default). An
    /// enumeration-domain utility (`Pick`/`Omit`/…) whose source
    /// argument is an OPEN generic instantiation (`PropsBase<T>` with
    /// `T` an unsubstituted type parameter) is open ⇒ the reducer keeps
    /// it a shallow carrier. A CLOSED source (concrete instantiation, or
    /// a finite object surface) is NOT open ⇒ it still materialises.
    /// Discriminating: an over-broad "builtin utility == carrier" L1
    /// would return `true` for the closed cases too and fail this test.
    #[test]
    fn utility_enumeration_domain_open_for_unbound_generic_closed_for_concrete() {
        use crate::semantic_query::{
            DeclIdentity, HashValue, SemanticNodeData, SurfaceView, TupleElement,
        };

        let host = VerterHost::new_standalone(Default::default());
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        let builtin_pick = DeclIdentity {
            canonical_id: Arc::from("__builtin__"),
            whole_hash: HashValue::default(),
            decl_name: Arc::from("Pick"),
        };
        // Keyspace argument — never inspected by the openness walk
        // (only argument 0, the enumeration domain, matters).
        let keys = graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::String));

        // OPEN: PropsBase<T> with T an unsubstituted type parameter.
        let tparam = graph.intern_node(SemanticNodeData::TypeParam {
            decl: DeclIdentity::synthetic("T"),
            param_index: 0,
            constraint: None,
            default: None,
            display_name: Arc::from("T"),
        });
        let props_base_open = graph.intern_node(SemanticNodeData::InstantiationRef {
            base: DeclIdentity {
                canonical_id: Arc::from("/types.ts"),
                whole_hash: HashValue::default(),
                decl_name: Arc::from("PropsBase"),
            },
            args: Arc::from(vec![tparam].into_boxed_slice()),
        });
        assert!(
            super::utility_enumeration_domain_is_open_or_unknown(
                &host,
                &builtin_pick,
                &[props_base_open, keys],
            ),
            "Pick<PropsBase<T>, …> over an unbound generic must be OPEN"
        );

        let concrete_elem =
            graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::Unknown));
        let concrete_array = graph.intern_node(SemanticNodeData::Array {
            element: concrete_elem,
            readonly: false,
        });

        // OPEN: PropsBase<UIMessage[]> — a NON-EMPTY all-concrete arg list
        // is NOT sufficient on its own. An instantiation closes only when
        // its target declaration EXISTS with satisfiable arity/defaults and
        // a body that closes under the bindings. Here no `PropsBase` decl is
        // seeded in this pure-unit host, so the target is UNRESOLVABLE ⇒
        // undecidable ⇒ OPEN — even with concrete args. The
        // resolvable-closed path (a real generic decl that materialises
        // path-precisely) is exercised end-to-end by the integration
        // fixtures in `component_meta_pick_omit_tests`.
        let props_base_concrete_unresolved =
            graph.intern_node(SemanticNodeData::InstantiationRef {
                base: DeclIdentity {
                    canonical_id: Arc::from("/types.ts"),
                    whole_hash: HashValue::default(),
                    decl_name: Arc::from("PropsBase"),
                },
                args: Arc::from(vec![concrete_array].into_boxed_slice()),
            });
        assert!(
            super::utility_enumeration_domain_is_open_or_unknown(
                &host,
                &builtin_pick,
                &[props_base_concrete_unresolved, keys],
            ),
            "Pick<PropsBase<UIMessage[]>, …> with concrete args but an UNRESOLVABLE target must \
             be OPEN — an instantiation is closed only when its target decl is resolvable with \
             a body that closes under the bindings"
        );

        // OPEN: a bare / under-applied generic alias — `InstantiationRef`
        // with EMPTY args — over a target whose prepared body is
        // unresolvable (no decl seeded) is undecidable ⇒ OPEN.
        let bare_alias_unresolved = graph.intern_node(SemanticNodeData::InstantiationRef {
            base: DeclIdentity {
                canonical_id: Arc::from("/types.ts"),
                whole_hash: HashValue::default(),
                decl_name: Arc::from("SlotProps"),
            },
            args: Arc::from(Vec::new().into_boxed_slice()),
        });
        assert!(
            super::utility_enumeration_domain_is_open_or_unknown(
                &host,
                &builtin_pick,
                &[bare_alias_unresolved, keys],
            ),
            "Pick<SlotProps, …> with EMPTY args over an unresolvable target must be OPEN \
             (no arg binds the body's free params)"
        );

        // CLOSED: a finite object surface domain.
        let closed_object = graph.intern_node(SemanticNodeData::Object(SurfaceView {
            members: Arc::from(Vec::new().into_boxed_slice()),
            call_signatures: Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        }));
        assert!(
            !super::utility_enumeration_domain_is_open_or_unknown(
                &host,
                &builtin_pick,
                &[closed_object, keys],
            ),
            "Pick<{{ … }}, …> over a finite object surface must be CLOSED"
        );

        // A NON-utility instantiation is never subject to this
        // carrier-stop, even with an open source argument.
        let not_a_utility = DeclIdentity {
            canonical_id: Arc::from("/types.ts"),
            whole_hash: HashValue::default(),
            decl_name: Arc::from("Lookup"),
        };
        assert!(
            !super::utility_enumeration_domain_is_open_or_unknown(
                &host,
                &not_a_utility,
                &[props_base_open, keys],
            ),
            "a non-enumeration-utility instantiation is not subject to L1 carrier-stop"
        );

        // Tuple domains are finite surfaces (concrete numeric/length key
        // space) — keep them closed.
        let tuple = graph.intern_node(SemanticNodeData::Tuple {
            elements: Arc::from(
                vec![TupleElement {
                    label: None,
                    value: concrete_elem,
                    optional: false,
                    rest: false,
                }]
                .into_boxed_slice(),
            ),
            readonly: false,
        });
        assert!(
            !super::utility_enumeration_domain_is_open_or_unknown(
                &host,
                &builtin_pick,
                &[tuple, keys]
            ),
            "a concrete tuple domain must be CLOSED"
        );
    }

    /// Keyspace-inspection invariant: the openness walk must inspect the
    /// KEYSPACE of an `IndexedAccess` / `Mapped` domain, not only the
    /// object / source. A domain `Source[OpenKey]` or
    /// `{ [K in OpenKeySpace]: V }` with a CONCRETE object but an OPEN key
    /// space is OPEN — an object/source-only inspection would wrongly
    /// judge it CLOSED and materialise an undecidable key set.
    #[test]
    fn utility_enumeration_domain_open_via_indexed_access_and_mapped_keyspace() {
        use crate::semantic_query::{
            DeclIdentity, HashValue, IndexKey, MapperKey, MapperKind, OptionalityMod, ReadonlyMod,
            SemanticNodeData, SurfaceView,
        };

        let host = VerterHost::new_standalone(Default::default());
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        let builtin_pick = DeclIdentity {
            canonical_id: Arc::from("__builtin__"),
            whole_hash: HashValue::default(),
            decl_name: Arc::from("Pick"),
        };
        let keys = graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::String));

        // CONCRETE object, OPEN type-param key.
        let concrete_object = graph.intern_node(SemanticNodeData::Object(SurfaceView {
            members: Arc::from(Vec::new().into_boxed_slice()),
            call_signatures: Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        }));
        let open_key = graph.intern_node(SemanticNodeData::TypeParam {
            decl: DeclIdentity::synthetic("K"),
            param_index: 0,
            constraint: None,
            default: None,
            display_name: Arc::from("K"),
        });

        // IndexedAccess { object: concrete, index: TypeNode(open K) }.
        let indexed_open_key = graph.intern_node(SemanticNodeData::IndexedAccess {
            object: concrete_object,
            index: IndexKey::TypeNode(open_key),
        });
        assert!(
            super::utility_enumeration_domain_is_open_or_unknown(
                &host,
                &builtin_pick,
                &[indexed_open_key, keys],
            ),
            "IndexedAccess with a concrete object but an OPEN type-param index key must be OPEN \
             (keyspace must be inspected, not just the object)"
        );

        // A literal-string index key over the same concrete object is CLOSED.
        let indexed_closed_key = graph.intern_node(SemanticNodeData::IndexedAccess {
            object: concrete_object,
            index: IndexKey::String(Arc::from("a")),
        });
        assert!(
            !super::utility_enumeration_domain_is_open_or_unknown(
                &host,
                &builtin_pick,
                &[indexed_closed_key, keys],
            ),
            "IndexedAccess with a concrete object and a literal-string index key must be CLOSED"
        );

        // Mapped { source: concrete, mapper.key_space: open OUTER T }. The
        // mapper's own binder is a DISTINCT node — it is BOUND inside the
        // mapper walk and must not be conflated with the open outer
        // parameter that makes the key space undecidable.
        let binder = graph.intern_node(SemanticNodeData::TypeParam {
            decl: DeclIdentity::synthetic("MapBinder"),
            param_index: 0,
            constraint: None,
            default: None,
            display_name: Arc::from("MapBinder"),
        });
        let mapped_open_keyspace = graph.intern_node(SemanticNodeData::Mapped {
            source: concrete_object,
            mapper: MapperKey {
                parameter_node: binder,
                key_space: open_key,
                value_expr: concrete_object,
                optionality: OptionalityMod::Keep,
                readonly: ReadonlyMod::Keep,
                name_remap: None,
                kind: MapperKind::Computed,
            },
        });
        assert!(
            super::utility_enumeration_domain_is_open_or_unknown(
                &host,
                &builtin_pick,
                &[mapped_open_keyspace, keys],
            ),
            "Mapped with a concrete source but an OPEN mapper key space must be OPEN \
             (the produced key set is undecidable; key_space must be inspected, not just source)"
        );
    }

    /// MappedTemplate key-remap coverage with the mapper binder BOUND
    /// (the binder is bound in EVERY walk — keyspace, value, remap):
    ///
    /// - a remap interpolating an OPEN OUTER parameter (`` `on${T}` ``)
    ///   over concrete source/key_space is OPEN — the produced (remapped)
    ///   key set depends on the open interpolant;
    /// - a remap interpolating ONLY the mapper's OWN binder
    ///   (`` `on${K}` `` over a finite key space) is a K-only transform —
    ///   CLOSED, decidable per key once `K` is bound;
    /// - a CONCRETE remap (no interpolant) is CLOSED.
    ///
    /// Discriminating: a remap walk that did NOT bind the binder would
    /// judge the K-only remap open (over-fire); one that bound the outer
    /// parameter too would judge the outer-interpolant remap closed
    /// (under-fire).
    #[test]
    fn utility_enumeration_domain_mapped_name_remap_binder_bound_outer_open() {
        use crate::semantic_query::{
            DeclIdentity, HashValue, MapperKey, MapperKind, OptionalityMod, ReadonlyMod,
            SemanticNodeData, SemanticNodeId, SurfaceView,
        };

        let host = VerterHost::new_standalone(Default::default());
        let graph = Arc::clone(host.project_type_store().semantic_graph());

        let builtin_pick = DeclIdentity {
            canonical_id: Arc::from("__builtin__"),
            whole_hash: HashValue::default(),
            decl_name: Arc::from("Pick"),
        };
        let keys = graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::String));

        let concrete_object = graph.intern_node(SemanticNodeData::Object(SurfaceView {
            members: Arc::from(Vec::new().into_boxed_slice()),
            call_signatures: Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        }));
        let concrete_key =
            graph.intern_node(SemanticNodeData::Primitive(SemanticPrimitiveKind::String));
        // The mapper's OWN binder `K` (bound) vs the open OUTER `T`.
        let binder_k = graph.intern_node(SemanticNodeData::TypeParam {
            decl: DeclIdentity::synthetic("K"),
            param_index: 0,
            constraint: None,
            default: None,
            display_name: Arc::from("K"),
        });
        let outer_t = graph.intern_node(SemanticNodeData::TypeParam {
            decl: DeclIdentity::synthetic("T"),
            param_index: 0,
            constraint: None,
            default: None,
            display_name: Arc::from("T"),
        });

        let make_mapped = |remap: SemanticNodeId| {
            graph.intern_node(SemanticNodeData::Mapped {
                source: concrete_object,
                mapper: MapperKey {
                    parameter_node: binder_k,
                    key_space: concrete_key,
                    value_expr: concrete_object,
                    optionality: OptionalityMod::Keep,
                    readonly: ReadonlyMod::Keep,
                    name_remap: Some(remap),
                    kind: MapperKind::Computed,
                },
            })
        };
        let template = |interpolant: SemanticNodeId| {
            graph.intern_node(SemanticNodeData::TemplateLiteral {
                quasis: Arc::from(
                    vec![Arc::<str>::from("on"), Arc::<str>::from("")].into_boxed_slice(),
                ),
                expressions: Arc::from(vec![interpolant].into_boxed_slice()),
            })
        };

        // OPEN: `` as `on${T}` `` — the remapped key set depends on the
        // open OUTER interpolant.
        assert!(
            super::utility_enumeration_domain_is_open_or_unknown(
                &host,
                &builtin_pick,
                &[make_mapped(template(outer_t)), keys],
            ),
            "Mapped with concrete source + key_space but an `as`-clause name-remap \
             interpolating an open OUTER parameter must be OPEN"
        );

        // CLOSED: `` as `on${K}` `` — interpolates ONLY the mapper's own
        // BOUND binder over a finite key space (a K-only transform).
        assert!(
            !super::utility_enumeration_domain_is_open_or_unknown(
                &host,
                &builtin_pick,
                &[make_mapped(template(binder_k)), keys],
            ),
            "Mapped whose `as`-clause name-remap interpolates ONLY the mapper's own bound \
             binder over a finite key space must stay CLOSED (the binder is bound in every \
             walk; a K-only remap is decidable per key)"
        );

        // CLOSED control: a CONCRETE name-remap (no interpolant).
        let closed_remap = graph.intern_node(SemanticNodeData::TemplateLiteral {
            quasis: Arc::from(vec![Arc::<str>::from("on")].into_boxed_slice()),
            expressions: Arc::from(Vec::new().into_boxed_slice()),
        });
        assert!(
            !super::utility_enumeration_domain_is_open_or_unknown(
                &host,
                &builtin_pick,
                &[make_mapped(closed_remap), keys],
            ),
            "Mapped with concrete source/key_space and a CONCRETE name-remap must stay CLOSED \
             (the name_remap arm must not over-fire)"
        );
    }
}
