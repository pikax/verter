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
//! (forward direction). The Step 6.1 invariant test
//! `semantic_node_to_type_expr_has_exactly_one_path` asserts exactly one
//! `fn raise_node_to_type_expr` exists in `crates/`.

use std::sync::Arc;

use rustc_hash::FxHashSet;
use verter_semantic::analysis::type_expr::TypeExpr;

use super::ProjectSemanticDispatch;
use crate::resolver_core::component_meta_query_engine::{
    projected_surface_to_type_expr, semantic_query_error_raw, surface_view_to_projected_surface,
    SEMANTIC_OBJECT_SURFACE,
};
use crate::semantic_query::{
    DepSignature, IndexKey, MapperKey, OptionalityMod, PathSegment,
    PrimitiveKind as SemanticPrimitiveKind, ProjectionMode, QueryError, QueryResult, ReadonlyMod,
    ResolveDeclKey, ScopeId, SemanticNodeData, SemanticNodeId, SemanticQueryKey, SurfaceMember,
    SurfaceView, TupleElement,
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
            mode: *mode,
        },
        SemanticQueryKey::IndexedAccess { base, index, mode } => SemanticQueryKey::ProjectPath {
            base: *base,
            path: Arc::from(vec![PathSegment::Index(index.clone())].into_boxed_slice()),
            mode: *mode,
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
    /// [`Self::raise_and_reduce`] (Step 6.1.A). This function alone is
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
                // Path C C11a — drop empty-object arms from the
                // Intersection projection. `Id<T> = {} & { [P in keyof T]: T[P] }`
                // and similar helper patterns lower to
                // Intersection([empty_object, mapped_object]); the empty
                // arm contributes nothing semantically (`{} & X ≡ X`) but
                // leaks through as a `TypeExpr::Unknown { raw:
                // SEMANTIC_OBJECT_SURFACE }` sentinel which breaks callers
                // that expect a pure Object at the projection boundary.
                // Dropping the semantically-vacuous arm here collapses
                // `{} & X → X` so imported-helper ui bindings materialise
                // cleanly instead of nested in Intersection([Unknown, Object]).
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
                use verter_semantic::analysis::type_expr::TupleElement;

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
            // C16: DeclPlaceholder → TypeExpr::Ref (replaces DeclAnchor).
            SemanticNodeData::Opaque(QueryError::DeclPlaceholder { name, .. }) => TypeExpr::Ref {
                name: std::sync::Arc::clone(name),
                type_arguments: verter_semantic::analysis::type_expr::empty_type_args(),
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
                // Path C C6a item 9c: presentational projection. Look
                // up the binder node by `mapper.parameter_node` and
                // read its `display_name` for the projected
                // `TypeExpr::Mapped { parameter }` field. C7's interner
                // dedups only structurally-identical binders, so the
                // representative's display_name is well-defined.
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
                    OptionalityMod::Add => {
                        verter_semantic::analysis::type_expr::MappedModifier::Add
                    }
                    OptionalityMod::Remove => {
                        verter_semantic::analysis::type_expr::MappedModifier::Remove
                    }
                    OptionalityMod::Keep => {
                        verter_semantic::analysis::type_expr::MappedModifier::None
                    }
                },
                readonly: match mapper.readonly {
                    ReadonlyMod::Add => verter_semantic::analysis::type_expr::MappedModifier::Add,
                    ReadonlyMod::Remove => {
                        verter_semantic::analysis::type_expr::MappedModifier::Remove
                    }
                    ReadonlyMod::Keep => verter_semantic::analysis::type_expr::MappedModifier::None,
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
                TypeExpr::TypeOf(verter_semantic::analysis::type_expr::ValueRef { path: segments })
            }
            SemanticNodeData::TypeParam {
                display_name,
                constraint,
                default,
                ..
            } => {
                // Cluster A: project `constraint` / `default` back
                // to `TypeExpr` so the round-trip preserves the declaration
                // shape. The `active` visited set guards against cyclic
                // constraint graphs (plan F7): when a TypeParam's
                // constraint or default transitively reaches this same
                // node, return `None` from the recursion and drop the
                // field rather than looping.
                //
                // Path C C6: the projected `TypeExpr::TypeParameter.name`
                // uses `display_name` — the human-readable parameter
                // name. `decl` / `param_index` are identity discriminators
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
                TypeExpr::TypeParameter(verter_semantic::analysis::type_expr::TypeParam {
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
            // Phase D §5.6 WIP-L / §3 Change L — canonical Function shape
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
            } => {
                use verter_semantic::analysis::type_expr::{
                    FunctionExpr, FunctionParam, TypeParam,
                };
                let parameters: Vec<FunctionParam> = params
                    .iter()
                    .filter_map(|p| {
                        Some(FunctionParam {
                            name: p.name.as_ref().map(|n| n.as_ref().to_string()),
                            ty: self.raise_node_to_type_expr_inner(p.ty, active)?,
                            optional: p.optional,
                            rest: p.rest,
                        })
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
                TypeExpr::Function(std::sync::Arc::new(FunctionExpr {
                    parameters,
                    return_type: return_ty,
                    type_parameters: type_params,
                }))
            }
            // D26 lazy carriers. DeclRef raises to a
            // bare `Ref { name }` with empty type arguments. Identity
            // (`canonical_id + whole_hash`) is encoded in the interning
            // scope, not in the projected TypeExpr — that's the lossy
            // direction of the Navigate-mode lazy lowering.
            SemanticNodeData::DeclRef { identity } => TypeExpr::Ref {
                name: std::sync::Arc::clone(&identity.decl_name),
                type_arguments: verter_semantic::analysis::type_expr::empty_type_args(),
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

    /// `execute` variant that returns the full [`CacheRead`] (D38).
    ///
    /// `ProjectSemanticDispatch::execute` (the [`SemanticQueryApi`] trait
    /// method) discards the dep-signature half of the cache read; this
    /// variant keeps it so callers like [`Self::raise_and_reduce`] can
    /// accumulate dep facts across nested dispatches and merge them into
    /// the session-layer `fact_versions` (Step 6.6.A).
    #[allow(dead_code)] // wired by Step 6.3 caller migration.
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
        // tests can dump the top-N most-dispatched keys (deferred per
        // §1.C.3 pending an InputMenu corpus fixture).
        #[cfg(test)]
        record_dispatch_key(&key);

        // Supplement §5.D.0 r17 — cold/warm split is recorded
        // inside `SemanticGraphStore::execute_cooperative` after
        // canonicalisation. The digest function in
        // `SemanticQueryKeyDigest::from_key` canonicalises the key
        // before hashing so caller-side probes (e.g.
        // `family_cold(&ProjectMember{..})`) read the same counter
        // as the canonical form (`ProjectPath{path: [Member(..)]}`)
        // the warm cache stores.

        // Mirror the canonicalisation done by `execute` so the cache key
        // identity is stable across the sugar variants.
        let key = match key {
            SemanticQueryKey::ProjectMember { base, member, mode } => {
                SemanticQueryKey::ProjectPath {
                    base,
                    path: Arc::from(vec![PathSegment::Member(member)].into_boxed_slice()),
                    mode,
                }
            }
            SemanticQueryKey::IndexedAccess { base, index, mode } => {
                SemanticQueryKey::ProjectPath {
                    base,
                    path: Arc::from(vec![PathSegment::Index(index)].into_boxed_slice()),
                    mode,
                }
            }
            SemanticQueryKey::NormalizeUnion { members } => SemanticQueryKey::NormalizeUnion {
                members: super::canonicalize_node_list(&members),
            },
            SemanticQueryKey::NormalizeIntersection { members } => {
                SemanticQueryKey::NormalizeIntersection {
                    members: super::canonicalize_node_list(&members),
                }
            }
            other => other,
        };

        let graph = Arc::clone(self.graph());
        let sentinel_key = key.clone();
        let sentinel = {
            let graph = Arc::clone(&graph);
            move || {
                if let SemanticQueryKey::Instantiate { base, .. } = &sentinel_key {
                    return graph.intern_node(SemanticNodeData::Opaque(QueryError::RecursiveRef {
                        name: Arc::clone(&base.decl_name),
                    }));
                }
                graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss))
            }
        };
        let key_for_build = key.clone();
        let build = move || match &key_for_build {
            SemanticQueryKey::ResolveDecl(decl_key) => self.build_resolve_decl(decl_key),
            SemanticQueryKey::TypeOf { value_root } => self.build_typeof(value_root),
            SemanticQueryKey::Instantiate {
                base,
                args,
                body_mode,
            } => self.build_instantiate(base, args, *body_mode),
            SemanticQueryKey::ProjectMember { base, member, mode } => {
                let path: Arc<[PathSegment]> =
                    Arc::from(vec![PathSegment::Member(Arc::clone(member))].into_boxed_slice());
                self.build_project_path(*base, &path, *mode)
            }
            SemanticQueryKey::IndexedAccess { base, index, mode } => {
                let path: Arc<[PathSegment]> =
                    Arc::from(vec![PathSegment::Index(index.clone())].into_boxed_slice());
                self.build_project_path(*base, &path, *mode)
            }
            SemanticQueryKey::ProjectPath { base, path, mode } => {
                self.build_project_path(*base, path, *mode)
            }
            SemanticQueryKey::KeyOf { base } => self.build_key_of(*base),
            SemanticQueryKey::MappedType { source, mapper } => {
                self.build_mapped_type(*source, mapper)
            }
            SemanticQueryKey::Conditional {
                check,
                extends,
                true_branch,
                false_branch,
                distributive,
            } => {
                self.build_conditional(*check, *extends, *true_branch, *false_branch, *distributive)
            }
            SemanticQueryKey::NormalizeUnion { members } => self.build_normalize_union(members),
            SemanticQueryKey::NormalizeIntersection { members } => {
                self.build_normalize_intersection(members)
            }
            SemanticQueryKey::ResolvedNamedType { key } => self.build_resolved_named_type(key),
            SemanticQueryKey::Relate { .. } => {
                let fence = self.project_generation_signature();
                (QueryResult::Error(QueryError::Miss), fence)
            }
            // Binding amendment — `ResolveMacroPayload`.
            SemanticQueryKey::ResolveMacroPayload {
                owner,
                macro_index,
                macro_kind,
                type_args,
                mode,
            } => {
                self.build_resolve_macro_payload(owner, *macro_index, *macro_kind, type_args, *mode)
            }
        };
        graph.execute_cooperative(key, sentinel, build)
    }

    /// Reduce a [`SemanticNodeId`] by dispatching the appropriate
    /// [`SemanticQueryKey`] for each operator-shape encountered in the
    /// graph subtree, then raise the fully-reduced graph node to a
    /// [`TypeExpr`].
    ///
    /// Operates GRAPH-NATIVE: walks [`SemanticNodeData`] via the
    /// graph's `node_data`, dispatches per shape, interns reduced
    /// shells via [`crate::semantic_query_memo::SemanticGraphStore::intern_preserving_scope`].
    /// No re-lowering of raised TypeExpr subtrees (Codex P0 #1).
    ///
    /// `mode` is threaded into nested dispatches and determines whether
    /// `DeclRef` / `InstantiationRef` reduce eagerly (Expanded) or stay
    /// terminal (Navigate).
    ///
    /// Returns a [`MaterializedTypeExpr`] carrying the producing
    /// `SemanticNodeId`, the raised `TypeExpr`, and the accumulated
    /// `DepSignature` (D31).
    #[allow(dead_code)] // wired by Step 6.3 caller migration.
    pub(crate) fn raise_and_reduce(
        &self,
        node: SemanticNodeId,
        mode: ProjectionMode,
    ) -> MaterializedTypeExpr {
        let mut state = ReduceState::default();
        let reduced = self.reduce_graph_node_iterative(node, mode, &mut state);
        let type_expr = self
            .raise_node_to_type_expr(reduced)
            .unwrap_or(TypeExpr::Unknown {
                raw: "<raise miss after reduction>".to_string(),
            });
        MaterializedTypeExpr {
            node_id: Some(reduced),
            type_expr,
            dep_signature: state.into_dep_signature(),
        }
    }

    /// Iterative graph-native reducer (D33 — stack-safe).
    ///
    /// Two-phase iteration:
    /// 1. **Top-down traversal**: walk every reachable `(node, mode)`
    ///    pair starting from `root`, recording the visit order in
    ///    `topo`. Cycles are broken via the `visited` set keyed by
    ///    `(SemanticNodeId, ProjectionMode)`.
    /// 2. **Bottom-up reduction**: process `topo` in reverse so children
    ///    are fully reduced before parents. For each node, look up the
    ///    `SemanticNodeData`, apply the per-shape rule, and record the
    ///    reduction in `mapping`. Parent rebuilds substitute child
    ///    reductions via `mapping`.
    ///
    /// Stack-safe for arbitrarily deep acyclic structures (≥5000 levels;
    /// verified by stack-safety regression fixtures in §4.1).
    #[allow(dead_code)] // wired by raise_and_reduce above.
    fn reduce_graph_node_iterative(
        &self,
        root: SemanticNodeId,
        mode: ProjectionMode,
        state: &mut ReduceState,
    ) -> SemanticNodeId {
        // Loop-5 instrumentation — count every iterative-reduction
        // entry. One `raise_and_reduce` produces exactly one of these.
        crate::loop5_instrumentation::RAISE_REDUCE_GRAPH_NODE_ITERATIVE_CALLS
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Collect every reachable `(node, mode)` pair in topo
        // order with a worklist. `visited` short-circuits cycles —
        // they reach a fixpoint at the first visit and are not re-pushed.
        let mut topo: Vec<SemanticNodeId> = Vec::new();
        let mut to_visit: Vec<SemanticNodeId> = vec![root];
        while let Some(node) = to_visit.pop() {
            if !state.visited.insert((node, mode)) {
                continue;
            }
            topo.push(node);
            let Some(data) = super::node_data_for(self.ctx, node) else {
                continue;
            };
            // Push children for traversal. Operator-shape children
            // (IndexedAccess.object, Conditional.check/extends/branches,
            // KeyOf.base, etc.) are reachable via their concrete
            // operands; the dispatch happens during reduction.
            push_children(&data, &mut to_visit);
        }

        // Process topo in reverse — children first. Each
        // (node, mode) reduces by looking at SemanticNodeData and
        // dispatching the per-shape key. The result is recorded in
        // `mapping`; subsequent parents look up child reductions there.
        for &node in topo.iter().rev() {
            let reduced = self.reduce_one(node, mode, state);
            state.mapping.insert(node, reduced);
        }
        state.mapping.get(&root).copied().unwrap_or(root)
    }

    /// Reduce a single node assuming all its children have already been
    /// reduced (their reductions live in `state.mapping`). Returns the
    /// reduced `SemanticNodeId`.
    ///
    /// Per-shape table:
    /// - Operator shapes (`IndexedAccess`, `KeyOf`, `Conditional`,
    ///   `Mapped`, `TypeOf`) dispatch the matching `SemanticQueryKey`.
    ///   `Value(reduced)` with `reduced != node` recurses; `Value(node)`
    ///   (deferred over a free type parameter) accepts the form.
    /// - `DeclRef` / `InstantiationRef`: in `Navigate` mode, terminal
    ///   except DeclRef which still follows aliases via D41. In
    ///   `Expanded`, dispatch `ResolveDecl` / `Instantiate`.
    /// - Composite shapes (`Object` / `Union` / `Intersection` / `Array` /
    ///   `Tuple` / `Function`) rebuild via `intern_preserving_scope`
    ///   when any child reduced; else return `node` unchanged.
    /// - `TemplateLiteral` / `Infer` / `Rest`-style hard-stops have no
    ///   dispatch variant and become `Unknown { raw: "<…>" }`.
    /// - Terminals (`Primitive` / `Literal` / `TypeParam` / `Opaque(…)`)
    ///   return `node` as-is.
    #[allow(dead_code)] // wired by reduce_graph_node_iterative above.
    fn reduce_one(
        &self,
        node: SemanticNodeId,
        mode: ProjectionMode,
        state: &mut ReduceState,
    ) -> SemanticNodeId {
        let Some(data) = super::node_data_for(self.ctx, node) else {
            return node;
        };
        match data.as_ref() {
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
            SemanticNodeData::Alias(target) => {
                state.mapping.get(target).copied().unwrap_or(*target)
            }

            // --- operator dispatches (mode-aware via underlying key) ---
            SemanticNodeData::IndexedAccess { object, index } => {
                let object = state.mapping.get(object).copied().unwrap_or(*object);
                let index = index.clone();
                self.dispatch_operator_with_recurse(
                    node,
                    SemanticQueryKey::IndexedAccess {
                        base: object,
                        index,
                        mode,
                    },
                    mode,
                    state,
                )
            }
            SemanticNodeData::KeyOf { base } => {
                let base = state.mapping.get(base).copied().unwrap_or(*base);
                self.dispatch_operator_with_recurse(
                    node,
                    SemanticQueryKey::KeyOf { base },
                    mode,
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
                let check = state.mapping.get(check).copied().unwrap_or(*check);
                let extends = state.mapping.get(extends).copied().unwrap_or(*extends);
                let true_branch = state
                    .mapping
                    .get(true_branch_ref)
                    .copied()
                    .unwrap_or(*true_branch_ref);
                let false_branch = state
                    .mapping
                    .get(false_branch_ref)
                    .copied()
                    .unwrap_or(*false_branch_ref);
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
                    mode,
                    state,
                )
            }
            SemanticNodeData::Mapped { source, mapper } => {
                let source = state.mapping.get(source).copied().unwrap_or(*source);
                // Re-key the mapper with reduced source nodes when those
                // were touched. The mapper carries `key_space` /
                // `value_expr` etc. — they're separate operator-graph
                // edges and may have been reduced independently.
                let mapper = remap_mapper(mapper, &state.mapping);
                self.dispatch_operator_with_recurse(
                    node,
                    SemanticQueryKey::MappedType { source, mapper },
                    mode,
                    state,
                )
            }
            SemanticNodeData::TypeOf { value_root, .. } => {
                let value_root = value_root.clone();
                self.dispatch_operator_with_recurse(
                    node,
                    SemanticQueryKey::TypeOf { value_root },
                    mode,
                    state,
                )
            }

            // --- lazy carriers (D26+D41) ---
            SemanticNodeData::DeclRef { identity } => {
                if matches!(mode, ProjectionMode::Navigate) {
                    // D41: Navigate follows alias chains because aliases
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
                self.dispatch_operator_with_recurse(node, key, mode, state)
            }
            SemanticNodeData::InstantiationRef { base, args } => {
                if matches!(mode, ProjectionMode::Navigate) {
                    // D41: InstantiationRef is TERMINAL in Navigate —
                    // generic application is a structural expansion.
                    return node;
                }
                let identity = base.clone();
                let args: Arc<[SemanticNodeId]> = Arc::from(
                    args.iter()
                        .map(|id| state.mapping.get(id).copied().unwrap_or(*id))
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                );
                let key = SemanticQueryKey::Instantiate {
                    base: identity,
                    args,
                    body_mode: mode,
                };
                self.dispatch_operator_with_recurse(node, key, mode, state)
            }

            // --- composite rebuilds via intern_preserving_scope ---
            SemanticNodeData::Object(_) => {
                // The Object surface carries pre-built SurfaceMember /
                // signature ids. Rebuild only if any child changed.
                rebuild_object(self, node, &state.mapping).unwrap_or(node)
            }
            SemanticNodeData::Union(arms) => {
                rebuild_union_or_intersection(
                    self,
                    node,
                    arms,
                    /* is_union */ true,
                    &state.mapping,
                )
                .unwrap_or(node)
            }
            SemanticNodeData::Intersection(arms) => {
                rebuild_union_or_intersection(
                    self,
                    node,
                    arms,
                    /* is_union */ false,
                    &state.mapping,
                )
                .unwrap_or(node)
            }
            SemanticNodeData::Array { element, readonly } => {
                let new_elem = state.mapping.get(element).copied().unwrap_or(*element);
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
                rebuild_tuple(self, node, elements, *readonly, &state.mapping).unwrap_or(node)
            }
            SemanticNodeData::Function {
                params,
                return_type,
                type_parameters,
            } => rebuild_function(
                self,
                node,
                params,
                *return_type,
                type_parameters,
                &state.mapping,
            )
            .unwrap_or(node),
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
        mode: ProjectionMode,
        state: &mut ReduceState,
    ) -> SemanticNodeId {
        // Loop-5 instrumentation — every operator-node dispatch issues
        // one `execute_read` (which routes through `execute_cooperative`).
        crate::loop5_instrumentation::DISPATCH_OPERATOR_WITH_RECURSE_CALLS
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let read = self.execute_read(key);
        state.merge_dep_signature(&read.dep_signature);
        match read.value {
            QueryResult::Value(result) => {
                if result == node {
                    node
                } else if let Some(&already_reduced) = state.mapping.get(&result) {
                    already_reduced
                } else if state.visited.insert((result, mode)) {
                    // The dispatch produced a new node that wasn't in
                    // our topo. Reduce it inline (single hop — its
                    // children stay graph-native and their own
                    // reduction is cheap because the visited-set
                    // dedups). Cycle protection: visited.insert above.
                    let recursed = self.reduce_one(result, mode, state);
                    state.mapping.insert(result, recursed);
                    recursed
                } else {
                    state.mapping.get(&result).copied().unwrap_or(result)
                }
            }
            QueryResult::Recursive(_id) => node,
            QueryResult::Error(_) => node,
        }
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

/// Push all child `SemanticNodeId` references from `data` onto
/// `worklist` (reducer traversal). Operator children, surface
/// members, signatures, conditional branches, etc. are all collected so
/// the topo order in reduces them before their parents.
#[allow(dead_code)] // wired by reduce_graph_node_iterative above.
fn push_children(data: &SemanticNodeData, worklist: &mut Vec<SemanticNodeId>) {
    match data {
        SemanticNodeData::Primitive(_)
        | SemanticNodeData::Literal(_)
        | SemanticNodeData::Opaque(_)
        | SemanticNodeData::Infer { .. }
        | SemanticNodeData::TemplateLiteral { .. }
        | SemanticNodeData::TypeOf { .. }
        | SemanticNodeData::VueMacroElements(_)
        | SemanticNodeData::DeclRef { .. } => {}
        SemanticNodeData::Alias(t) => worklist.push(*t),
        SemanticNodeData::Object(view) => push_surface_children(view, worklist),
        SemanticNodeData::Union(arms) | SemanticNodeData::Intersection(arms) => {
            for arm in arms.iter() {
                worklist.push(*arm);
            }
        }
        SemanticNodeData::Array { element, .. } => worklist.push(*element),
        SemanticNodeData::Tuple { elements, .. } => {
            for el in elements.iter() {
                worklist.push(el.value);
            }
        }
        SemanticNodeData::KeyOf { base } => worklist.push(*base),
        SemanticNodeData::IndexedAccess { object, index } => {
            worklist.push(*object);
            if let IndexKey::TypeNode(n) = index {
                worklist.push(*n);
            }
        }
        SemanticNodeData::Mapped { source, mapper } => {
            worklist.push(*source);
            worklist.push(mapper.parameter_node);
            worklist.push(mapper.key_space);
            worklist.push(mapper.value_expr);
            if let Some(name_remap) = mapper.name_remap {
                worklist.push(name_remap);
            }
        }
        SemanticNodeData::TypeParam {
            constraint,
            default,
            ..
        } => {
            if let Some(c) = constraint {
                worklist.push(*c);
            }
            if let Some(d) = default {
                worklist.push(*d);
            }
        }
        SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            ..
        } => {
            worklist.push(*check);
            worklist.push(*extends);
            worklist.push(*true_branch_ref);
            worklist.push(*false_branch_ref);
        }
        SemanticNodeData::Function {
            params,
            return_type,
            type_parameters,
        } => {
            for p in params.iter() {
                worklist.push(p.ty);
            }
            worklist.push(*return_type);
            for tp in type_parameters.iter() {
                if let Some(c) = tp.constraint {
                    worklist.push(c);
                }
                if let Some(d) = tp.default {
                    worklist.push(d);
                }
            }
        }
        SemanticNodeData::InstantiationRef { args, .. } => {
            for arg in args.iter() {
                worklist.push(*arg);
            }
        }
    }
}

#[allow(dead_code)] // wired by push_children above.
fn push_surface_children(view: &SurfaceView, worklist: &mut Vec<SemanticNodeId>) {
    for member in view.members.iter() {
        worklist.push(member.value);
    }
    for sig in view.call_signatures.iter() {
        worklist.push(*sig);
    }
    for sig in view.construct_signatures.iter() {
        worklist.push(*sig);
    }
    for sig in view.index_signatures.iter() {
        worklist.push(sig.key_type);
        worklist.push(sig.value_type);
    }
    if let Some(ks) = view.keyspace {
        worklist.push(ks);
    }
}

/// Re-key a `MapperKey` using `mapping` (substituting reduced child node
/// ids). Returns a fresh key with substituted ids; structural fields
/// (`optionality`, `readonly`) are preserved.
#[allow(dead_code)] // wired by reduce_one above.
fn remap_mapper(
    mapper: &MapperKey,
    mapping: &rustc_hash::FxHashMap<SemanticNodeId, SemanticNodeId>,
) -> MapperKey {
    let mut new_mapper = mapper.clone();
    new_mapper.parameter_node = mapping
        .get(&mapper.parameter_node)
        .copied()
        .unwrap_or(mapper.parameter_node);
    new_mapper.key_space = mapping
        .get(&mapper.key_space)
        .copied()
        .unwrap_or(mapper.key_space);
    new_mapper.value_expr = mapping
        .get(&mapper.value_expr)
        .copied()
        .unwrap_or(mapper.value_expr);
    new_mapper.name_remap = mapper
        .name_remap
        .map(|id| mapping.get(&id).copied().unwrap_or(id));
    new_mapper
}

#[allow(dead_code)] // wired by reduce_one above.
fn rebuild_object(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    mapping: &rustc_hash::FxHashMap<SemanticNodeId, SemanticNodeId>,
) -> Option<SemanticNodeId> {
    let data = super::node_data_for(dispatch.ctx, node)?;
    let SemanticNodeData::Object(view) = data.as_ref() else {
        return None;
    };
    let mut changed = false;
    let new_members: Arc<[SurfaceMember]> = {
        let mut out: Vec<SurfaceMember> = Vec::with_capacity(view.members.len());
        for m in view.members.iter() {
            let new_value = mapping.get(&m.value).copied().unwrap_or(m.value);
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
            let new_sig = mapping.get(sig).copied().unwrap_or(*sig);
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
            let new_sig = mapping.get(sig).copied().unwrap_or(*sig);
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
    mapping: &rustc_hash::FxHashMap<SemanticNodeId, SemanticNodeId>,
) -> Option<SemanticNodeId> {
    let mut changed = false;
    let new_arms: Vec<SemanticNodeId> = arms
        .iter()
        .map(|arm| {
            let new = mapping.get(arm).copied().unwrap_or(*arm);
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
    mapping: &rustc_hash::FxHashMap<SemanticNodeId, SemanticNodeId>,
) -> Option<SemanticNodeId> {
    let mut changed = false;
    let new_elements: Vec<TupleElement> = elements
        .iter()
        .map(|el| {
            let new_value = mapping.get(&el.value).copied().unwrap_or(el.value);
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
fn rebuild_function(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    params: &Arc<[crate::semantic_query::FunctionParam]>,
    return_type: SemanticNodeId,
    type_parameters: &Arc<[crate::semantic_query::TypeParamDecl]>,
    mapping: &rustc_hash::FxHashMap<SemanticNodeId, SemanticNodeId>,
) -> Option<SemanticNodeId> {
    let mut changed = false;
    let new_params: Vec<crate::semantic_query::FunctionParam> = params
        .iter()
        .map(|p| {
            let new_ty = mapping.get(&p.ty).copied().unwrap_or(p.ty);
            if new_ty != p.ty {
                changed = true;
            }
            crate::semantic_query::FunctionParam {
                ty: new_ty,
                ..p.clone()
            }
        })
        .collect();
    let new_return = mapping.get(&return_type).copied().unwrap_or(return_type);
    if new_return != return_type {
        changed = true;
    }
    let new_type_params: Vec<crate::semantic_query::TypeParamDecl> = type_parameters
        .iter()
        .map(|tp| {
            let new_constraint = tp.constraint.map(|c| mapping.get(&c).copied().unwrap_or(c));
            let new_default = tp.default.map(|d| mapping.get(&d).copied().unwrap_or(d));
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
        },
    ))
}

/// Materialization-grade result returned by [`raise_and_reduce`] and the
/// session-layer `materialize_*` wrapper (D31).
///
/// Step 6.1.A introduces the type; Step 6.3 wires the
/// session-layer caller `materialize_component_meta_type_expr_until_stable`
/// to return this struct.
///
/// - `node_id`: `Some(id)` for dispatch-produced entries; `None` for
///   synthetic / inline-annotation entries that bypass the dispatch
///   path. Captured at materialization time for `SurfaceNodeIdentities`
///   population (D32).
/// - `type_expr`: the raised final form.
/// - `dep_signature`: accumulated fence signatures from all dispatch
///   calls inside reduction. Session merges into
///   `ResolvedComponentMetaState.fact_versions` before publish (Step
///   6.6.A).
#[allow(dead_code)] // wired by Step 6.3 caller migration.
#[derive(Debug, Clone)]
pub struct MaterializedTypeExpr {
    pub node_id: Option<SemanticNodeId>,
    pub type_expr: TypeExpr,
    pub dep_signature: DepSignature,
}

#[allow(dead_code)] // wired by raise_and_reduce above.
#[derive(Default)]
struct ReduceState {
    visited: FxHashSet<(SemanticNodeId, ProjectionMode)>,
    mapping: rustc_hash::FxHashMap<SemanticNodeId, SemanticNodeId>,
    dep_facts: Vec<(Arc<str>, crate::semantic_query::DepVersion)>,
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

fn semantic_primitive_to_type_expr(kind: SemanticPrimitiveKind) -> TypeExpr {
    use verter_semantic::analysis::type_expr::PrimitiveName;

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
    use verter_semantic::analysis::type_expr::TypeExpr;

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

        let TypeExpr::IndexedAccess { index, .. } = expr else {
            panic!("expected IndexedAccess expr, got {expr:?}");
        };
        assert_eq!(
            *index,
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

    /// FAIL-FIRST (Step 6.1.A): preserves deferred operator over a free
    /// `TypeParameter`. Pre-fix: `raise_and_reduce` doesn't exist.
    /// Post-fix: `KeyOf(TypeParameter)` survives because dispatch returns
    /// the same shape (deferred-form policy).
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

    /// FAIL-FIRST (Step 6.1.A + D33): the iterative reducer terminates
    /// even when the visited set is the only termination signal. Pre-fix:
    /// the reducer doesn't exist. Post-fix: visited set short-circuits
    /// the cycle and returns the alias body.
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

    /// FAIL-FIRST (Step 6.1.A): hard-stop for `TemplateLiteral` —
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

        match materialized.type_expr {
            TypeExpr::Unknown { raw } => {
                assert!(
                    raw.contains("template literal"),
                    "template literal hard-stop should mention the operator, got {raw:?}"
                );
            }
            other => panic!("expected Unknown hard-stop, got {other:?}"),
        }
    }

    /// FAIL-FIRST (Step 6.1.A + D26): Navigate-mode keeps DeclRef
    /// terminal. Pre-fix: lazy carriers don't exist. Post-fix: a
    /// freshly-interned `DeclRef` raises to a bare `Ref { name }` with
    /// empty type arguments.
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

        match materialized.type_expr {
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

        let TypeExpr::IndexedAccess { index, .. } = expr else {
            panic!("expected IndexedAccess expr, got {expr:?}");
        };
        assert_eq!(
            *index,
            TypeExpr::string_literal("key"),
            "string index keys should serialize as string literals",
        );
    }
}
