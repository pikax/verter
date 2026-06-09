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

use rustc_hash::FxHashSet;
use verter_type_expr::TypeExpr;

use super::ProjectSemanticDispatch;
use crate::instant::Instant;
use crate::resolver_core::component_meta_query_engine::{
    projected_surface_to_type_expr, semantic_query_error_raw, surface_view_to_projected_surface,
    SEMANTIC_OBJECT_SURFACE,
};
use crate::semantic_query::{
    DepSignature, IndexKey, MapperKey, OptionalityMod, PrimitiveKind as SemanticPrimitiveKind,
    ProjectionMode, ProjectionReductionContext, QueryError, QueryResult, ReadonlyMod,
    ReductionDemand, ResolveDeclKey, ScopeId, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
    SurfaceMember, SurfaceView, TupleElement,
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
#[allow(dead_code, reason = "wired in B1 / I §10.8 dispatch-count assertions")]
pub(crate) struct DispatchTraceGuard;

#[cfg(test)]
impl Drop for DispatchTraceGuard {
    fn drop(&mut self) {
        DISPATCH_TRACE.with(|t| t.borrow_mut().clear());
    }
}

#[cfg(test)]
#[allow(dead_code, reason = "wired in B1 / I §10.8 dispatch-count assertions")]
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
                    .collect();
                if arms.len() == 1 {
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
            SemanticNodeData::Object(surface) => projected_surface_to_type_expr(
                &surface_view_to_projected_surface(self.ctx, surface),
            )
            .unwrap_or(TypeExpr::Unknown {
                raw: SEMANTIC_OBJECT_SURFACE.to_string(),
            }),
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
            SemanticNodeData::TypeOf { value_root, path } => {
                let mut segments = value_root
                    .name
                    .split('.')
                    .map(|segment| segment.to_string())
                    .collect::<Vec<_>>();
                segments.extend(path.iter().map(|segment| segment.as_ref().to_string()));
                TypeExpr::TypeOf(verter_type_expr::ValueRef { path: segments })
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
        })
    }

    fn index_key_to_type_expr(
        &self,
        index: &IndexKey,
        active: &mut FxHashSet<SemanticNodeId>,
    ) -> Option<TypeExpr> {
        Some(match index {
            IndexKey::String(text) => TypeExpr::string_literal(text.as_ref()),
            IndexKey::Number(number) => TypeExpr::number_literal(*number as f64),
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
            | SemanticNodeData::TypeOf { .. }
            | SemanticNodeData::VueMacroElements(_)
            | SemanticNodeData::DeclRef { .. } => {}
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
                // `ensure_indexed_ready`.
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
            | SemanticNodeData::VueMacroElements(_) => node,

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
            SemanticNodeData::TypeOf { value_root, .. } => {
                let value_root = value_root.clone();
                self.dispatch_operator_with_recurse(
                    node,
                    SemanticQueryKey::TypeOf { value_root },
                    context,
                    state,
                )
            }

            // --- lazy carriers ---
            SemanticNodeData::DeclRef { identity } => {
                if matches!(mode, ProjectionMode::Navigate) {
                    // Navigate follows alias chains because aliases
                    // are semantically transparent. Dispatch and recurse
                    // — same as Expanded for DeclRef.
                }
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
                if matches!(mode, ProjectionMode::Navigate) {
                    // L1 (Shallow-By-Default): an object-filter utility
                    // (`Pick`/`Omit`) whose enumeration domain
                    // (source argument) is OPEN — an unbound generic, an
                    // open conditional/mapped/indexed/keyof, an
                    // instantiation over open args, or an unresolved
                    // declaration — STAYS a shallow carrier under
                    // `Published(Navigate)` instead of materialising its
                    // source. Materialising an open source degenerates
                    // into full cross-file generic expansion (the
                    // `ChatMessages.vue` `Pick<PropsBase<T>, …>` storm).
                    // A CLOSED source (finite object surface / concrete
                    // instantiation) still materialises path-precisely.
                    //
                    // The domain args are substituted through `mapping`
                    // first so a bound type argument (a closed
                    // instantiation) is judged closed and still reduces.
                    let resolved_args: Vec<SemanticNodeId> = args
                        .iter()
                        .map(|id| state.mapping.get(&(*id, context)).copied().unwrap_or(*id))
                        .collect();
                    if utility_enumeration_domain_is_open_or_unknown(self.ctx, base, &resolved_args)
                    {
                        return node;
                    }
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

/// Budget for the bounded alias-chain resolver
/// ([`prepared_decl_body_is_closed`]). Each transparent alias hop
/// (`type Foo = Bar`) decrements it; a legitimate alias chain terminates
/// at a closed object surface in a handful of hops. Exhaustion ⇒
/// conservatively OPEN.
const ALIAS_CHAIN_HOP_BUDGET: u32 = 32;

/// Whether the prepared declaration `(canonical, name)` resolves to a
/// CLOSED finite object surface through a BOUNDED, transparent alias chain.
///
/// Replaces the previous single-hop `is_closed_object_body(&prepared.body)`
/// shortcut, which mis-classified a perfectly legitimate closed alias chain
/// (`type Foo = Bar; type Bar = { bar: string }`) as OPEN — the source of
/// `Pick<Foo, 'bar'>` failing to materialise. The walk follows a body that
/// is a bare `TypeExpr::Ref` (no type arguments — a transparent alias) to
/// the referenced declaration via the prepared decl's `name_resolution`
/// context (NO route discovery, NO reducer, NO `execute_read` — prepared
/// declaration cache reads ONLY), bounded by a hop budget + a visited set.
///
/// CLOSED ⇒ the chain terminates at a finite object surface (or a finite
/// intersection of object surfaces). OPEN ⇒ the chain reaches an
/// operator/conditional/mapped body, a `Ref` with type arguments (a generic
/// instantiation whose closedness depends on its args — not a transparent
/// alias hop), an unresolved name, a missing prepared decl, a cycle, or
/// exhausts the hop budget.
fn prepared_decl_body_is_closed(
    ctx: &dyn crate::resolver_core::ResolverContext,
    canonical_id: &str,
    decl_name: &str,
    budget: &mut u32,
    visited: &mut FxHashSet<(Arc<str>, Arc<str>)>,
) -> bool {
    let key = (Arc::<str>::from(canonical_id), Arc::<str>::from(decl_name));
    if !visited.insert(key) {
        // Cycle on the alias frontier — not a proven closed surface.
        return false;
    }
    if *budget == 0 {
        return false;
    }
    *budget -= 1;

    let Some(prepared) = ctx.prepared_type_decl(canonical_id, decl_name) else {
        return false;
    };
    body_type_expr_is_closed(ctx, &prepared, &prepared.body, budget, visited)
}

/// Inspect a prepared declaration BODY `TypeExpr` for closedness, following
/// transparent alias hops (`type Foo = Bar`) through `prepared`'s
/// `name_resolution` context. Bounded by the same budget + visited set as
/// [`prepared_decl_body_is_closed`].
fn body_type_expr_is_closed(
    ctx: &dyn crate::resolver_core::ResolverContext,
    prepared: &verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl,
    body: &TypeExpr,
    budget: &mut u32,
    visited: &mut FxHashSet<(Arc<str>, Arc<str>)>,
) -> bool {
    match body {
        TypeExpr::Object(_) => true,
        TypeExpr::Intersection(arms) => arms
            .iter()
            .all(|arm| body_type_expr_is_closed(ctx, prepared, arm, budget, visited)),
        TypeExpr::Parenthesized(inner) => {
            body_type_expr_is_closed(ctx, prepared, inner, budget, visited)
        }
        // A bare `Ref` with NO type arguments is a transparent alias hop:
        // follow it to the referenced declaration. A `Ref` WITH type
        // arguments is a generic instantiation whose closedness depends on
        // its arguments and substituted body — NOT a transparent alias —
        // so it is not followed here (conservatively OPEN).
        TypeExpr::Ref {
            name,
            type_arguments,
        } if type_arguments.is_empty() => {
            let Some(target) = prepared.name_resolution.get(name.as_ref()) else {
                return false;
            };
            prepared_decl_body_is_closed(
                ctx,
                &target.canonical_id,
                &target.symbol_name,
                budget,
                visited,
            )
        }
        _ => false,
    }
}

/// Whether the instantiation `base<args…>` (with `arg_count` ALREADY
/// verified-closed type arguments) can be PROVEN to close.
///
/// Replaces the unsound "non-empty, all-concrete args ⇒ CLOSED" shortcut.
/// An instantiation closes only when:
///   1. the target declaration exists (a prepared decl is available),
///   2. its arity is satisfiable: `required_params <= arg_count <= total_params`
///      (the trailing `total_params - arg_count` params fall back to defaults),
///   3. every default supplied for an unfilled param is itself closed, and
///   4. the prepared body is closed under those bindings — a type-parameter
///      reference counts as BOUND (closed) because its arg/default was
///      verified closed; any other open structure (operator/conditional/
///      mapped body, unresolved `Ref`, instantiation over an open target)
///      opens the instantiation.
///
/// Bounded by the same hop budget + a fresh visited set; prepared-decl cache
/// reads ONLY — no reducer, no substitution, no `execute_read`.
fn prepared_instantiation_is_closed(
    ctx: &dyn crate::resolver_core::ResolverContext,
    base: &crate::semantic_query::DeclIdentity,
    arg_count: usize,
    budget: &mut u32,
) -> bool {
    let Some(prepared) =
        ctx.prepared_type_decl(base.canonical_id.as_ref(), base.decl_name.as_ref())
    else {
        // Unresolved target ⇒ undecidable ⇒ not closed.
        return false;
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

    // Bind set: every declared type-parameter name is bound (the first
    // `arg_count` to verified-closed args, the rest to defaults). Defaults
    // for unfilled params must themselves be closed.
    let mut chain_visited: FxHashSet<(Arc<str>, Arc<str>)> = FxHashSet::default();
    for param in prepared.type_parameters.iter().skip(arg_count) {
        let Some(default) = param.default.as_deref() else {
            // No default for an unfilled param — guarded by the arity check
            // above, but stay conservative.
            return false;
        };
        if !body_type_expr_is_closed(ctx, &prepared, default, budget, &mut chain_visited) {
            return false;
        }
    }

    let bound_params: FxHashSet<&str> = prepared
        .type_parameters
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    instantiated_body_is_closed(ctx, &prepared, &prepared.body, &bound_params, budget)
}

/// Whether a prepared declaration body is closed under a set of BOUND
/// type-parameter names. A reference to a bound parameter counts as closed
/// (its binding arg/default was verified closed by the caller). Any other
/// structure recurses; an unresolved free `Ref`, a generic instantiation
/// over an open/unresolvable target, or an operator body over a free param
/// opens the body. Bounded by `budget`.
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

fn instantiated_body_is_closed(
    ctx: &dyn crate::resolver_core::ResolverContext,
    prepared: &verter_semantic::analysis::type_solver::prepared::PreparedTypeDecl,
    body: &TypeExpr,
    bound_params: &FxHashSet<&str>,
    budget: &mut u32,
) -> bool {
    if *budget == 0 {
        return false;
    }
    *budget -= 1;
    match body {
        TypeExpr::Object(_) | TypeExpr::Primitive(_) | TypeExpr::Literal(_) => true,
        TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) => {
            instantiated_body_is_closed(ctx, prepared, inner, bound_params, budget)
        }
        TypeExpr::Array { element, .. } => {
            instantiated_body_is_closed(ctx, prepared, element, bound_params, budget)
        }
        TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => arms
            .iter()
            .all(|arm| instantiated_body_is_closed(ctx, prepared, arm, bound_params, budget)),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .all(|e| instantiated_body_is_closed(ctx, prepared, &e.ty, bound_params, budget)),
        // A bare type-parameter reference is a LEAF type variable: under a
        // fully-concrete instantiation it is bound to a verified-closed
        // type, and it carries no own enumerable key space that the OUTER
        // object-filter would expand. Closed regardless of whether the
        // name is one of THIS decl's params (it may also be a conditional
        // `infer` binding introduced inside the body).
        TypeExpr::TypeParameter(_) => true,
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            // A bare `Ref` (no type arguments) is a leaf reference. If it
            // names a bound parameter / inference binding it is closed; if
            // it resolves to a declaration, require that declaration's
            // bounded alias chain to close; an unresolvable bare name is
            // open.
            if type_arguments.is_empty() {
                if bound_params.contains(name.as_ref()) {
                    return true;
                }
                return match prepared.name_resolution.get(name.as_ref()) {
                    Some(target) => {
                        let mut chain_visited: FxHashSet<(Arc<str>, Arc<str>)> =
                            FxHashSet::default();
                        prepared_decl_body_is_closed(
                            ctx,
                            &target.canonical_id,
                            &target.symbol_name,
                            budget,
                            &mut chain_visited,
                        )
                    }
                    // A bare unresolved `Ref` — a free name that is neither
                    // a bound type parameter nor resolvable through this
                    // decl's name-resolution — is OPEN (undecidable). It is
                    // NOT an `infer`-introduced binding (those lower to the
                    // dedicated `TypeExpr::Infer` arm, which returns closed);
                    // treating an unresolved free ref as a closed leaf would
                    // wrongly prove an open body closed and let the L1
                    // carrier-stop materialise an undecidable surface.
                    None => false,
                };
            }
            // A `Ref` WITH type arguments is a generic instantiation. Every
            // type argument must close first.
            if !type_arguments
                .iter()
                .all(|a| instantiated_body_is_closed(ctx, prepared, a, bound_params, budget))
            {
                return false;
            }
            // A `contains_key` membership test is INSUFFICIENT — a resolvable
            // name does NOT imply the instantiation closes. Resolve the
            // target through this decl's name-resolution, then run the SAME
            // bounded, prepared-decl-only verification used at the top level
            // (`prepared_instantiation_is_closed`): the target decl must
            // exist, its arity over `type_arguments.len()` must be
            // satisfiable, every default for an unfilled param must close,
            // and the substituted body must close. An unresolvable target is
            // open. Bounded + cache-read-only (no reducer / no execute_read).
            let Some(target) = prepared.name_resolution.get(name.as_ref()) else {
                return false;
            };
            let target_identity = crate::semantic_query::DeclIdentity {
                canonical_id: Arc::from(target.canonical_id.as_str()),
                whole_hash: crate::semantic_query::HashValue::default(),
                decl_name: Arc::from(target.symbol_name.as_str()),
            };
            prepared_instantiation_is_closed(ctx, &target_identity, type_arguments.len(), budget)
        }
        // Operator bodies (conditional / indexed-access / keyof / mapped)
        // are CLOSED *under these bindings* when every operand is closed.
        // With a NON-EMPTY all-concrete arg list every free type parameter
        // is bound to a verified-closed type, so a conditional with a
        // bound-closed check/extends is decidable (it reduces to one
        // branch during real materialisation) and the operator does not
        // leave an open key space. We require BOTH branches / both operands
        // closed (a sound over-approximation that avoids reducing here).
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            // `infer X` bindings in the `extends` clause bind `X` for the
            // conditional's branches (TS semantics). Harvest them and treat
            // them as bound leaves in the branch recursion so a branch
            // referencing an inferred name (lowered as a bare `Ref`, e.g.
            // `ChatMessageProps<M, D, U>` where `M`/`D`/`U` are inferred in
            // `extends UIMessage<infer M, infer D, infer U>`) is NOT wrongly
            // opened by the bare-unresolved-`Ref` rule.
            let mut branch_bound: FxHashSet<&str> = bound_params.clone();
            collect_infer_names(extends, &mut branch_bound);
            collect_infer_names(check, &mut branch_bound);
            instantiated_body_is_closed(ctx, prepared, check, bound_params, budget)
                && instantiated_body_is_closed(ctx, prepared, extends, bound_params, budget)
                && instantiated_body_is_closed(ctx, prepared, true_type, &branch_bound, budget)
                && instantiated_body_is_closed(ctx, prepared, false_type, &branch_bound, budget)
        }
        TypeExpr::IndexedAccess { object, index } => {
            instantiated_body_is_closed(ctx, prepared, object, bound_params, budget)
                && instantiated_body_is_closed(ctx, prepared, index, bound_params, budget)
        }
        TypeExpr::KeyOf(inner) => {
            instantiated_body_is_closed(ctx, prepared, inner, bound_params, budget)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            instantiated_body_is_closed(ctx, prepared, source, bound_params, budget)
                && instantiated_body_is_closed(ctx, prepared, value, bound_params, budget)
                && name_type.as_deref().is_none_or(|n| {
                    instantiated_body_is_closed(ctx, prepared, n, bound_params, budget)
                })
        }
        // `infer T` is a conditional-inference binding placeholder, not an
        // unbound generic — it does not leave an open key space.
        TypeExpr::Infer { .. } => true,
        // `typeof x`, recursive refs, template literals, and any shape not
        // modelled above stay conservatively not-provably-closed.
        _ => false,
    }
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
    use verter_semantic::analysis::type_solver::builtin::BuiltinUtility;
    if base.canonical_id.as_ref() != "__builtin__" {
        return None;
    }
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
    match BuiltinUtility::from_name(base.decl_name.as_ref())? {
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
fn utility_enumeration_domain_is_open_or_unknown(
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
    let mut budget = ENUMERATION_DOMAIN_OPENNESS_NODE_BUDGET;
    let mut visited = FxHashSet::default();
    enumeration_domain_node_is_open(ctx, domain, &mut budget, &mut visited)
}

/// Bounded typed-IR walk deciding whether `node` (a utility enumeration
/// domain) is OPEN — i.e. its key space still depends on unsubstituted
/// generic structure or cannot be proven a finite object surface.
///
/// OPEN ⇒ reaches an unsubstituted `TypeParam`, an `Opaque`
/// (resolution miss / degraded), a conditional whose check/extends is
/// open, an `IndexedAccess`/`KeyOf`/`Mapped` over an open operand, an
/// instantiation with an open type argument, an under-applied /
/// arity-unsatisfied / open-bodied `InstantiationRef`, an unresolved or
/// open-bodied `DeclRef` alias chain, or exhausts the walk budget. CLOSED
/// ⇒ a finite object/primitive/literal/function/array/tuple surface, a
/// finite union/intersection of closed arms, or a `DeclRef` whose bounded
/// alias chain terminates at a closed object surface. `Infer` is a
/// conditional-inference binding placeholder, NOT an unbound generic — it
/// does NOT open the domain on its own.
fn enumeration_domain_node_is_open(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: SemanticNodeId,
    budget: &mut u32,
    visited: &mut FxHashSet<SemanticNodeId>,
) -> bool {
    if !visited.insert(node) {
        // Back-edge on the current walk frontier: contributes no new
        // openness signal. Treat as closed for this revisit.
        return false;
    }
    if *budget == 0 {
        // Walk budget exhausted ⇒ conservatively undecidable ⇒ open.
        return true;
    }
    *budget -= 1;

    let Some(data) = super::node_data_for(ctx, node) else {
        // Unresolved / un-interned node ⇒ open.
        return true;
    };
    match data.as_ref() {
        // --- definitively open leaves ---
        // An unsubstituted `TypeParam` is an unbound generic — the
        // keyspace depends on it. An `Opaque` is a resolution miss /
        // degraded node. Both make the domain open.
        SemanticNodeData::TypeParam { .. } | SemanticNodeData::Opaque(_) => true,

        // `Infer` is a conditional-inference BINDING placeholder
        // (`extends UIMessage<infer M, …>`), NOT an unbound generic. A
        // conditional with a concrete `check` and `infer`-bearing
        // `extends` is decidable, so `Infer` alone does NOT open the
        // domain — only an unsubstituted `TypeParam` reached through the
        // `check`/`extends`/source does.
        SemanticNodeData::Infer { .. } => false,

        // --- operator shapes: open iff an open operand is reached ---
        SemanticNodeData::Conditional { check, extends, .. } => {
            enumeration_domain_node_is_open(ctx, *check, budget, visited)
                || enumeration_domain_node_is_open(ctx, *extends, budget, visited)
        }
        SemanticNodeData::KeyOf { base } => {
            enumeration_domain_node_is_open(ctx, *base, budget, visited)
        }
        // An indexed access is open when EITHER the object OR the index key
        // space is open. A `TypeParam`-keyed access (`T[K]` with open `K`)
        // is just as undecidable as an open object — inspecting only the
        // object would miss `Source[OpenKey]` source domains.
        SemanticNodeData::IndexedAccess { object, index } => {
            enumeration_domain_node_is_open(ctx, *object, budget, visited)
                || index_key_is_open(ctx, index, budget, visited)
        }
        // A mapped type is open when its source OR its mapper key space (or
        // an `as`-clause remap) is open. The produced key set of
        // `{ [K in OpenKeySpace]: V }` is undecidable even if `source` is
        // concrete, so inspecting only `source` would miss an open keyspace.
        SemanticNodeData::Mapped { source, mapper } => {
            enumeration_domain_node_is_open(ctx, *source, budget, visited)
                || enumeration_domain_node_is_open(ctx, mapper.key_space, budget, visited)
                || mapper
                    .name_remap
                    .is_some_and(|n| enumeration_domain_node_is_open(ctx, n, budget, visited))
        }
        SemanticNodeData::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .any(|e| enumeration_domain_node_is_open(ctx, *e, budget, visited)),
        // An instantiation is open when ANY type argument is open, OR the
        // target declaration cannot be PROVEN to close under those args.
        //
        // The previous "non-empty, all-concrete args ⇒ CLOSED" shortcut was
        // unsound: a target whose declaration is missing, arity-mismatched,
        // or whose body stays operator-shaped under the supplied bindings is
        // NOT closed even with concrete args. Closedness is now decided by
        // [`prepared_instantiation_is_closed`]: the target decl must exist,
        // its arity/defaults must be satisfiable by `args`, every supplied
        // default must be closed, and the prepared body must be closed under
        // those bindings (type-parameter references count as bound since
        // their arg/default was verified closed). The bare-alias /
        // under-applied case (`args.is_empty()`) is the `args.len() == 0`
        // instance of the same check. Bounded, prepared-decl cache reads
        // only — no reducer, no substitution, no `execute_read`.
        SemanticNodeData::InstantiationRef { base, args } => {
            if args
                .iter()
                .any(|a| enumeration_domain_node_is_open(ctx, *a, budget, visited))
            {
                return true;
            }
            !prepared_instantiation_is_closed(ctx, base, args.len(), budget)
        }

        // --- carriers we can follow one transparent hop ---
        SemanticNodeData::Alias(target) => {
            enumeration_domain_node_is_open(ctx, *target, budget, visited)
        }
        SemanticNodeData::DeclRef { identity } => {
            // A resolved declaration whose BOUNDED transparent alias chain
            // terminates at a finite closed-object surface is CLOSED;
            // anything else (an alias to an operator/conditional body, an
            // unresolved name, a cycle, or budget exhaustion) is open. The
            // single-hop `is_closed_object_body` shortcut would mis-classify
            // a legitimate closed alias chain
            // (`type Foo = Bar; type Bar = { bar: string }`) as OPEN; the
            // bounded chain resolver follows the transparent hops via the
            // prepared decl's `name_resolution` (cache reads only, no
            // reducer / `execute_read`).
            let mut chain_visited: FxHashSet<(Arc<str>, Arc<str>)> = FxHashSet::default();
            let mut hop_budget = ALIAS_CHAIN_HOP_BUDGET;
            !prepared_decl_body_is_closed(
                ctx,
                identity.canonical_id.as_ref(),
                identity.decl_name.as_ref(),
                &mut hop_budget,
                &mut chain_visited,
            )
        }

        // --- finite surfaces: the key space is enumerable ---
        SemanticNodeData::Object(_)
        | SemanticNodeData::Primitive(_)
        | SemanticNodeData::Literal(_)
        | SemanticNodeData::Function { .. }
        | SemanticNodeData::Array { .. }
        | SemanticNodeData::Tuple { .. }
        | SemanticNodeData::VueMacroElements(_) => false,

        // --- composites: open iff any arm is open ---
        SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => arms
            .iter()
            .any(|a| enumeration_domain_node_is_open(ctx, *a, budget, visited)),
        SemanticNodeData::MergedDecl { contributors } => contributors
            .iter()
            .any(|a| enumeration_domain_node_is_open(ctx, *a, budget, visited)),

        // --- unmodelled-for-this-walk shapes: conservatively open ---
        SemanticNodeData::TypeOf { .. } => true,
    }
}

/// Whether an [`IndexKey`] index operand is OPEN. Literal string / number
/// keys are concrete (closed); a `TypeNode` key is open iff its referenced
/// node is open under the same bounded walk.
fn index_key_is_open(
    ctx: &dyn crate::resolver_core::ResolverContext,
    index: &crate::semantic_query::IndexKey,
    budget: &mut u32,
    visited: &mut FxHashSet<SemanticNodeId>,
) -> bool {
    use crate::semantic_query::IndexKey;
    match index {
        IndexKey::String(_) | IndexKey::Number(_) => false,
        IndexKey::TypeNode(node) => enumeration_domain_node_is_open(ctx, *node, budget, visited),
    }
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
            index: IndexKey::Number(7),
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

        // OPEN (tightening 4): a bare / under-applied generic alias —
        // `InstantiationRef` with EMPTY args — over a target whose prepared
        // body is unresolvable (no decl seeded) is undecidable ⇒ OPEN.
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
             (tightening 4: no arg binds the body's free params)"
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

    /// Tightening 2 + 3 discriminator: the openness walk must inspect the
    /// KEYSPACE of an `IndexedAccess` / `Mapped` domain, not only the
    /// object / source. A domain `Source[OpenKey]` or
    /// `{ [K in OpenKeySpace]: V }` with a CONCRETE object but an OPEN key
    /// space is OPEN — pre-tightening (object/source-only inspection) it
    /// was wrongly judged CLOSED.
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

        // Mapped { source: concrete, mapper.key_space: open K }.
        let mapped_open_keyspace = graph.intern_node(SemanticNodeData::Mapped {
            source: concrete_object,
            mapper: MapperKey {
                parameter_node: open_key,
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
}
