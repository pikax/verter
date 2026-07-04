//! Fixed authored-shape locator lowering — the sealed [`LocatorShapeCtx`],
//! the carrier-only lowering entry
//! [`ProjectSemanticDispatch::lower_type_expr_for_locator_shape`], the
//! `SemanticQueryKey::LowerLocator` cold build, and the session-private
//! [`ProjectSemanticDispatch::lower_locator`] provider.
//!
//! ## Sealed context — capability split, not convention
//!
//! [`LocatorShapeCtx`] is a DISTINCT typed lowering context carrying ONLY
//! what fixed authored-shape lowering needs: the locator's authored-position
//! lexical scope plus the type-parameter binder environment. It NEITHER
//! contains NOR converts to a
//! [`ProjectionReductionContext`](crate::semantic_query::ProjectionReductionContext)
//! — no `From`/`Into`, no `AsRef`, no `Deref`, no accessor yields one — so
//! the reducing lowering entry (`shallow_lower_type_expr_with_context`,
//! which REQUIRES a `ProjectionReductionContext`) is unreachable from the
//! locator path BY TYPE. The trybuild fixtures
//! (`locator_shape_ctx_no_prc_conversion.rs` /
//! `locator_reducing_lowerer_not_nameable.rs`) pin the negative.
//!
//! ## Carrier-only, role-free lowering
//!
//! The entry interns the fixed authored SHAPE and nothing more:
//!
//! - operator positions (conditional / mapped / `keyof` / indexed access)
//!   ALWAYS intern DEFERRED carriers — branch selection, mapped
//!   materialisation, indexed-access projection, and member projection
//!   NEVER execute on the locator path;
//! - reference heads resolve IDENTITY ONLY: a resolvable name interns a
//!   `DeclRef` (bare) / `InstantiationRef` (applied) carrier; an unshadowed
//!   global lib head (`Promise`, the builtin utilities) interns the
//!   `__builtin__` `InstantiationRef` carrier; an unresolvable name stays a
//!   `BareRef` carrier — never an executed `ResolveDecl` / `Instantiate`;
//! - declared type parameters stay `TypeParam` shells (the key is strictly
//!   unsubstituted; substituted demands route through `Instantiate { args }`);
//! - the produced nodes are ROLE-FREE: object members carry the neutral
//!   `MacroOwnBodyStamp::NEUTRAL` / `MergeRoleStamp::NEUTRAL` stamps — the
//!   ONLY stamp values this path can construct. The stamp types' inner fields
//!   are private, so minting a NON-neutral value requires either a
//!   `ProjectionReductionContext` witness (`own_body_stamp` / `role_stamp` /
//!   `stamp_role` / `with_merge_role`) or the analyzed-macro-kind witness
//!   (`MacroOwnBodyStamp::from_macro_kind`). Those mint methods are `pub` and
//!   witness-gated — NOT visibility-sealed (an in-crate OR downstream caller
//!   holding a `ProjectionReductionContext`, e.g. via the `pub`
//!   `ProjectionReductionContext::published`, can mint) — so the locator seal
//!   is a THREADING seal, not a visibility one: the locator lowering entry
//!   receives a sealed `LocatorShapeCtx` that neither IS nor YIELDS a
//!   `ProjectionReductionContext`, and the path holds no analyzed macro kind,
//!   so NEITHER witness is reachable here. A role-stamped locator shape node
//!   is therefore a COMPILE error ON THIS PATH — pinned externally by the
//!   `member_role_stamps_not_mintable_without_witness` compile-fail fixture
//!   (E0308/E0423) and behaviorally by the
//!   `locator_shape_nodes_exclude_caller_relative_stamps` discriminator — not
//!   a convention. Caller-relative provenance / merge-role stamping is
//!   projection-time work applied to the fetched shape — never shape-node
//!   identity — so one reusable body-shape family exists per locator/env.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_type_expr::locators::AuthoredBodyLocator;
use verter_type_expr::{FunctionExpr, LiteralValue, MappedModifier, ObjectMember, TypeExpr};

use super::{empty_signature, map_primitive_name, ProjectSemanticDispatch};
use crate::decl_body_memo::DerefedBodyShape;
use crate::locator_identity::{
    semantic_space_for_locator_space, LocatorLoweringKey, ParseEnvHash, ResolveEnvHash,
};
use verter_semantic::analysis::type_solver::host::ResolvedRootIdentity;

use crate::resolver_core::bare_name_resolve::{
    resolve_bare_name_in_scope, DeclarationScopePayload,
};
use crate::semantic_query::{
    DeclIdentity, FunctionParam, HashValue, IndexKey, IndexSignature, MacroOwnBodyStamp, MapperKey,
    MapperKind, MergeRoleStamp, NodeScopeId, OptionalityMod, PrimitiveKind, QueryError,
    QueryResult, ReadonlyMod, ScopeId, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
    SurfaceMember, SurfaceView, SyntheticBindingId, TupleElement, TypeParamDecl, ValueRootKey,
};

/// The anchor declaration's prepared source, held so its
/// `name_resolution` map can be borrowed for the shape-lowering pass —
/// whichever prepared family the locator anchor names (a cached type /
/// value declaration, or an augmentation-scoped contribution prepared in
/// the anchor file's own context, exactly as the augmentation stitch
/// prepares it).
enum AnchorPreparedDecl {
    Type(Arc<verter_semantic::analysis::type_solver::PreparedTypeDecl>),
    Value(Arc<verter_semantic::analysis::type_solver::PreparedValueDecl>),
    Augmentation(Box<verter_semantic::analysis::type_solver::PreparedTypeDecl>),
}

impl AnchorPreparedDecl {
    fn name_resolution(&self) -> &FxHashMap<String, ResolvedRootIdentity> {
        match self {
            Self::Type(prepared) => &prepared.name_resolution,
            Self::Value(prepared) => &prepared.name_resolution,
            Self::Augmentation(prepared) => &prepared.name_resolution,
        }
    }
}

/// One lexical binder frame of the locator-shape lowering: declared
/// type-parameter / `infer` / mapped-binder names in scope at one nesting
/// level, each mapped to its interned binder node.
#[derive(Debug, Default, Clone)]
pub(crate) struct LocatorBinderFrame {
    names: FxHashMap<Arc<str>, SemanticNodeId>,
}

impl LocatorBinderFrame {
    /// Bind a syntactic binder name to its interned binder node.
    pub(crate) fn bind(&mut self, name: Arc<str>, node: SemanticNodeId) {
        self.names.insert(name, node);
    }

    fn lookup(&self, name: &str) -> Option<SemanticNodeId> {
        self.names.get(name).copied()
    }
}

/// The SEALED locator-shape lowering context: the authored position's
/// lexical scope + type-parameter binder environment + the declaration's
/// own bare-name resolution inputs — and NOTHING else.
///
/// Fields are PRIVATE and construction is in-crate only
/// ([`LocatorShapeCtx::new`]), so an outside unit can neither forge one nor
/// extract anything from it. Deliberately: no `Hash` / `Eq` (never a cache
/// key), and no conversion of any form to a
/// [`ProjectionReductionContext`](crate::semantic_query::ProjectionReductionContext)
/// — the non-reduction guarantee is encoded by TYPE/CAPABILITY.
///
/// `name_resolution` / `scope_payload` are IDENTITY inputs, not reduction
/// capability: reference heads must carrier-resolve to the SAME
/// `(canonical, symbol)` identity the reducing path resolves — the
/// declaration's own import/namespace-aware `name_resolution` map first,
/// then the payload-aware in-scope resolver — never a scope-less top-level
/// lookup (a declaration-local / namespace / import binding shadowing a
/// top-level symbol would otherwise cache the WRONG `DeclRef` /
/// `InstantiationRef` identity under `LowerLocator`).
#[derive(Debug, Clone, Copy)]
pub struct LocatorShapeCtx<'a> {
    /// The authored position's lexical scope (owning canonical + content
    /// generation).
    scope: &'a NodeScopeId,
    /// Innermost-last stack of binder frames; lookup scans from the top
    /// (last) frame outward so an inner binder shadows an outer one.
    binders: &'a [LocatorBinderFrame],
    /// The declaration's own bare-name → root-identity map (import /
    /// namespace-sibling aware) — the SAME fast path the reducing entry
    /// consults first. `None` when no prepared declaration exists for the
    /// anchor (the in-scope resolver below is then the whole authority).
    name_resolution: Option<&'a FxHashMap<String, ResolvedRootIdentity>>,
    /// The anchor scope's declaration-scope payload (scope-local names +
    /// import bindings), threaded to the shared in-scope resolver.
    scope_payload: Option<&'a DeclarationScopePayload>,
}

impl<'a> LocatorShapeCtx<'a> {
    /// Compose the sealed locator-shape context over the authored
    /// position's lexical `scope`, its type-parameter binder frames, and
    /// the declaration's bare-name resolution inputs.
    pub(crate) fn new(
        scope: &'a NodeScopeId,
        binders: &'a [LocatorBinderFrame],
        name_resolution: Option<&'a FxHashMap<String, ResolvedRootIdentity>>,
        scope_payload: Option<&'a DeclarationScopePayload>,
    ) -> Self {
        Self {
            scope,
            binders,
            name_resolution,
            scope_payload,
        }
    }
}

/// The module-internal working context of one locator-shape lowering pass.
#[derive(Clone, Copy)]
struct ShapeLowerCtx<'a> {
    scope: &'a NodeScopeId,
    binders: &'a [LocatorBinderFrame],
    name_resolution: Option<&'a FxHashMap<String, ResolvedRootIdentity>>,
    scope_payload: Option<&'a DeclarationScopePayload>,
}

impl<'a> ShapeLowerCtx<'a> {
    /// Swap the binder stack (the surrounding scope + resolution inputs are
    /// preserved) — used when a function's own generics / a mapper binder /
    /// an `infer` frame extends the stack for a sub-position.
    fn with_binders<'b>(&self, binders: &'b [LocatorBinderFrame]) -> ShapeLowerCtx<'b>
    where
        'a: 'b,
    {
        ShapeLowerCtx {
            scope: self.scope,
            binders,
            name_resolution: self.name_resolution,
            scope_payload: self.scope_payload,
        }
    }

    /// The binder node `name` stands for, scanning innermost-last outward.
    fn lookup_binder(&self, name: &str) -> Option<SemanticNodeId> {
        self.binders
            .iter()
            .rev()
            .find_map(|frame| frame.lookup(name))
    }
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// The carrier-only locator-shape lowering entry: intern the FIXED
    /// authored shape of `expr` under the sealed [`LocatorShapeCtx`].
    ///
    /// See the module docs for the full contract: deferred operator
    /// carriers, identity-only reference resolution, `TypeParam` shells,
    /// role-free member stamps. This entry never dispatches a
    /// `SemanticQueryKey` and never accepts a reducing context.
    pub(crate) fn lower_type_expr_for_locator_shape(
        &self,
        expr: &TypeExpr,
        ctx: &LocatorShapeCtx<'_>,
    ) -> SemanticNodeId {
        let work = ShapeLowerCtx {
            scope: ctx.scope,
            binders: ctx.binders,
            name_resolution: ctx.name_resolution,
            scope_payload: ctx.scope_payload,
        };
        self.lower_locator_shape_node(expr, &work)
    }

    /// Lower one node of the fixed authored shape. Recursive over the
    /// typed IR; every arm either interns a structural shell, a deferred
    /// operator carrier, or an identity carrier — never a reduction.
    fn lower_locator_shape_node(&self, expr: &TypeExpr, ctx: &ShapeLowerCtx<'_>) -> SemanticNodeId {
        let graph = self.graph();
        let scope = ctx.scope;
        match expr {
            // -- Structural terminals --
            TypeExpr::Primitive(name) => graph.intern_node_with_scope(
                SemanticNodeData::Primitive(map_primitive_name(*name)),
                scope.clone(),
            ),
            TypeExpr::Literal(value) => graph
                .intern_node_with_scope(SemanticNodeData::Literal(value.clone()), scope.clone()),

            // -- Composite structural shells --
            TypeExpr::Union(arms) => {
                let ids = self.lower_locator_shape_args(arms, ctx);
                if ids.is_empty() {
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                } else if ids.len() == 1 {
                    ids[0]
                } else {
                    graph.intern_node_with_scope(SemanticNodeData::Union(ids), scope.clone())
                }
            }
            TypeExpr::Intersection(arms) => {
                let ids = self.lower_locator_shape_args(arms, ctx);
                if ids.is_empty() {
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                } else if ids.len() == 1 {
                    ids[0]
                } else {
                    graph.intern_node_with_scope(SemanticNodeData::Intersection(ids), scope.clone())
                }
            }
            TypeExpr::Array { element, readonly } => {
                let element = self.lower_locator_shape_node(element, ctx);
                graph.intern_node_with_scope(
                    SemanticNodeData::Array {
                        element,
                        readonly: *readonly,
                    },
                    scope.clone(),
                )
            }
            // The plain tuple shell, per-element label / optional / rest
            // preserved verbatim. No variadic-spread normalization — that is
            // a reduction; open rest elements survive as authored.
            TypeExpr::Tuple { elements, readonly } => {
                let lowered: Vec<TupleElement> = elements
                    .iter()
                    .map(|el| TupleElement {
                        label: el.label.as_deref().map(Arc::<str>::from),
                        value: self.lower_locator_shape_node(&el.ty, ctx),
                        optional: el.optional,
                        rest: el.rest,
                    })
                    .collect();
                graph.intern_node_with_scope(
                    SemanticNodeData::Tuple {
                        elements: Arc::from(lowered.into_boxed_slice()),
                        readonly: *readonly,
                    },
                    scope.clone(),
                )
            }
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => {
                let quasis: Arc<[Arc<str>]> =
                    quasis.iter().map(|q| Arc::from(q.as_str())).collect();
                let expressions = self.lower_locator_shape_args(expressions, ctx);
                graph.intern_node_with_scope(
                    SemanticNodeData::TemplateLiteral {
                        quasis,
                        expressions,
                    },
                    scope.clone(),
                )
            }
            // Parenthesized types are structurally transparent.
            TypeExpr::Parenthesized(inner) => self.lower_locator_shape_node(inner, ctx),
            // A standalone rest outside tuple context is structurally
            // transparent (tuple-rest fidelity rides `TupleElement.rest`).
            TypeExpr::Rest(inner) => self.lower_locator_shape_node(inner, ctx),

            // -- Deferred operator shells (NEVER reduced) --
            TypeExpr::KeyOf(operand) => {
                let base = self.lower_locator_shape_node(operand, ctx);
                graph.intern_node_with_scope(SemanticNodeData::KeyOf { base }, scope.clone())
            }
            TypeExpr::IndexedAccess { object, index } => {
                let object = self.lower_locator_shape_node(object, ctx);
                let index = match index.as_ref() {
                    TypeExpr::Literal(LiteralValue::String(s)) => {
                        IndexKey::String(Arc::from(s.as_str()))
                    }
                    TypeExpr::Literal(LiteralValue::Number(n)) => {
                        match crate::semantic_query::index_key::integer_convention_index_key(*n) {
                            Some(i) => IndexKey::Number(i),
                            None => IndexKey::TypeNode(self.lower_locator_shape_node(index, ctx)),
                        }
                    }
                    _ => IndexKey::TypeNode(self.lower_locator_shape_node(index, ctx)),
                };
                graph.intern_node_with_scope(
                    SemanticNodeData::IndexedAccess { object, index },
                    scope.clone(),
                )
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                let check_id = self.lower_locator_shape_node(check, ctx);
                let extends_id = self.lower_locator_shape_node(extends, ctx);
                // `infer P` names introduced by the `extends` clause bind for
                // the TRUE branch only (TS scoping). Collect them from the
                // LOWERED extends node via the shared graph-side collector so
                // a true-branch `Ref { name: "P" }` resolves to the SAME
                // `Infer` binder node instead of leaking as a `BareRef`.
                let mut infer_env: FxHashMap<String, SemanticNodeId> = FxHashMap::default();
                let mut visited = rustc_hash::FxHashSet::default();
                self.collect_infer_bindings_into_env(extends_id, &mut infer_env, &mut visited);
                let true_id = if infer_env.is_empty() {
                    self.lower_locator_shape_node(true_type, ctx)
                } else {
                    let mut infer_frame = LocatorBinderFrame::default();
                    for (name, node) in infer_env {
                        infer_frame.bind(Arc::from(name.as_str()), node);
                    }
                    let mut frames: Vec<LocatorBinderFrame> = ctx.binders.to_vec();
                    frames.push(infer_frame);
                    let true_ctx = ctx.with_binders(&frames);
                    self.lower_locator_shape_node(true_type, &true_ctx)
                };
                let false_id = self.lower_locator_shape_node(false_type, ctx);
                let distributive = matches!(
                    graph.node_data(check_id).as_deref(),
                    Some(SemanticNodeData::TypeParam { .. })
                );
                graph.intern_node_with_scope(
                    SemanticNodeData::Conditional {
                        check: check_id,
                        extends: extends_id,
                        true_branch_ref: true_id,
                        false_branch_ref: false_id,
                        distributive,
                    },
                    scope.clone(),
                )
            }
            // Deferred mapped-type shell — the per-key value surface is
            // NEVER enumerated here.
            TypeExpr::Mapped {
                parameter,
                source,
                value,
                optional,
                readonly,
                name_type,
                ..
            } => {
                let mapper_display_name: Arc<str> = Arc::from(parameter.as_str());
                let mapper_decl = DeclIdentity::from_scope(scope, Arc::from("<mapper-param>"));
                // The mapper binder ordinal comes from the host-owned
                // registry — the SAME identity authority the reducing
                // lowering entry consults — so two lowerings of the same
                // source mapper share one binder identity while distinct
                // mappers in one file stay distinct.
                let fingerprint = crate::mapper_binder_registry::MapperFingerprint::from_components(
                    source,
                    value,
                    *optional,
                    *readonly,
                    name_type.as_ref(),
                );
                let mapper_ordinal = self
                    .ctx
                    .project_type_store()
                    .mapper_binder_registry()
                    .ordinal_for(&mapper_decl.canonical_id, &mapper_display_name, fingerprint);
                let parameter_node = graph.intern_node_with_scope(
                    SemanticNodeData::TypeParam {
                        decl: mapper_decl,
                        param_index: mapper_ordinal,
                        constraint: None,
                        default: None,
                        display_name: Arc::clone(&mapper_display_name),
                    },
                    scope.clone(),
                );
                let mut mapper_frame = LocatorBinderFrame::default();
                mapper_frame.bind(Arc::clone(&mapper_display_name), parameter_node);
                let mut frames: Vec<LocatorBinderFrame> = ctx.binders.to_vec();
                frames.push(mapper_frame);
                let body_ctx = ctx.with_binders(&frames);

                let (source_node, key_space) = match source.as_ref() {
                    TypeExpr::KeyOf(inner) => {
                        let inner_id = self.lower_locator_shape_node(inner, ctx);
                        let key_space = graph.intern_node_with_scope(
                            SemanticNodeData::KeyOf { base: inner_id },
                            scope.clone(),
                        );
                        (inner_id, key_space)
                    }
                    _ => {
                        let lowered = self.lower_locator_shape_node(source, ctx);
                        (lowered, lowered)
                    }
                };
                let value_expr = self.lower_locator_shape_node(value, &body_ctx);
                let name_remap = name_type
                    .as_deref()
                    .map(|nt| self.lower_locator_shape_node(nt, &body_ctx));
                let optionality = match optional {
                    MappedModifier::Add => OptionalityMod::Add,
                    MappedModifier::Remove => OptionalityMod::Remove,
                    MappedModifier::None => OptionalityMod::Keep,
                };
                let readonly = match readonly {
                    MappedModifier::Add => ReadonlyMod::Add,
                    MappedModifier::Remove => ReadonlyMod::Remove,
                    MappedModifier::None => ReadonlyMod::Keep,
                };
                let kind =
                    MapperKind::classify_value_expr(graph, value_expr, source_node, parameter_node);
                graph.intern_node_with_scope(
                    SemanticNodeData::Mapped {
                        source: source_node,
                        mapper: MapperKey {
                            parameter_node,
                            key_space,
                            value_expr,
                            optionality,
                            readonly,
                            name_remap,
                            kind,
                        },
                    },
                    scope.clone(),
                )
            }

            // -- Function / constructor signatures --
            TypeExpr::Function(func) => self.lower_locator_shape_function(func, ctx),
            TypeExpr::ConstructorType(func) => {
                let signature = self.lower_locator_shape_function(func, ctx);
                graph.intern_node_with_scope(
                    SemanticNodeData::ConstructorType { signature },
                    scope.clone(),
                )
            }

            // -- Declared type parameters stay SHELLS --
            TypeExpr::TypeParameter(param) => {
                if let Some(binder) = ctx.lookup_binder(&param.name) {
                    return binder;
                }
                let constraint = param
                    .constraint
                    .as_deref()
                    .map(|c| self.lower_locator_shape_node(c, ctx));
                let default = param
                    .default
                    .as_deref()
                    .map(|d| self.lower_locator_shape_node(d, ctx));
                let display_name: Arc<str> = Arc::from(param.name.as_str());
                let decl = DeclIdentity::from_scope(scope, Arc::clone(&display_name));
                graph.intern_node_with_scope(
                    SemanticNodeData::TypeParam {
                        decl,
                        param_index: 0,
                        constraint,
                        default,
                        display_name,
                    },
                    scope.clone(),
                )
            }

            // -- Named reference: identity resolution ONLY --
            TypeExpr::Ref {
                name,
                type_arguments,
            } => self.resolve_locator_ref_head(name, type_arguments, ctx),

            TypeExpr::Infer { name } => graph.intern_node_with_scope(
                SemanticNodeData::Infer {
                    name: Arc::from(name.as_str()),
                },
                scope.clone(),
            ),
            // Raw fallback — display/compat carrier, never a control signal.
            TypeExpr::Unknown { raw } => graph.intern_node_with_scope(
                SemanticNodeData::RawFallback {
                    raw: Arc::from(raw.as_str()),
                },
                scope.clone(),
            ),
            TypeExpr::SyntheticSlotBinding(key) => graph.intern_node_with_scope(
                SemanticNodeData::SyntheticBinding {
                    id: SyntheticBindingId::from_carrier_key(key),
                    value_node: key.value_node,
                },
                scope.clone(),
            ),
            // `typeof value.path<args>` stays the deferred TypeOf carrier —
            // never a `build_typeof` execution.
            TypeExpr::TypeOf(value_ref) => {
                let Some((root, rest)) = value_ref.path.split_first() else {
                    return self.opaque(QueryError::Miss);
                };
                let root_scope = match scope {
                    NodeScopeId::File { canonical_id, .. } => ScopeId {
                        canonical_id: Arc::clone(canonical_id),
                        local_scope: None,
                    },
                    NodeScopeId::Global => return self.opaque(QueryError::Miss),
                };
                let value_root = ValueRootKey {
                    scope: root_scope,
                    name: Arc::from(root.as_str()),
                };
                let path: Arc<[Arc<str>]> = rest.iter().map(|s| Arc::from(s.as_str())).collect();
                let type_args = self.lower_locator_shape_args(&value_ref.type_args, ctx);
                graph.intern_node_with_scope(
                    SemanticNodeData::new_typeof(value_root, path, type_args),
                    scope.clone(),
                )
            }
            // Dynamic-import reference stays the deferred ImportType carrier
            // — module resolution is a demand-time concern.
            TypeExpr::ImportType {
                specifier,
                qualifier,
                typeof_query,
                type_arguments,
            } => {
                let type_args = self.lower_locator_shape_args(type_arguments, ctx);
                graph.intern_node_with_scope(
                    SemanticNodeData::new_import_type(
                        Arc::clone(specifier),
                        Arc::clone(qualifier),
                        type_args,
                        *typeof_query,
                    ),
                    scope.clone(),
                )
            }
            // A solver-minted recursive back-edge never appears in authored
            // memo-derefed typed IR; fail closed.
            TypeExpr::RecursiveRef { .. } => self.opaque(QueryError::Miss),

            // -- Object surface: ROLE-FREE member stamps --
            TypeExpr::Object(obj) => {
                let declaration_origin = scope.canonical_file();
                let mut members: Vec<SurfaceMember> = Vec::new();
                let mut call_signatures: Vec<SemanticNodeId> = Vec::new();
                let mut construct_signatures: Vec<SemanticNodeId> = Vec::new();
                let mut index_signatures: Vec<IndexSignature> = Vec::new();
                for member in &obj.properties {
                    match member {
                        ObjectMember::Property(prop) => members.push(SurfaceMember {
                            name: Arc::from(prop.name.as_str()),
                            value: self.lower_locator_shape_node(&prop.ty, ctx),
                            optional: prop.optional,
                            readonly: prop.readonly,
                            is_method: false,
                            visibility: prop.visibility,
                            spans: prop.spans,
                            declaration_origin: declaration_origin.clone(),
                            // ROLE-FREE shape identity: the locator shape
                            // never carries a caller-relative provenance or
                            // merge role — those are projection-time stamps
                            // applied to the fetched shape, never node
                            // identity. NEUTRAL is the ONLY stamp this path
                            // can construct: the non-neutral producers
                            // require a `ProjectionReductionContext`
                            // witness, and the sealed `LocatorShapeCtx`
                            // neither contains nor converts to one.
                            declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
                            merge_role: MergeRoleStamp::NEUTRAL,
                        }),
                        ObjectMember::Method(method) => {
                            let function_expr =
                                TypeExpr::Function(Arc::new(method.function.clone()));
                            members.push(SurfaceMember {
                                name: Arc::from(method.name.as_str()),
                                value: self.lower_locator_shape_node(&function_expr, ctx),
                                optional: method.optional,
                                readonly: false,
                                is_method: true,
                                visibility: method.visibility,
                                spans: method.spans,
                                declaration_origin: declaration_origin.clone(),
                                declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
                                merge_role: MergeRoleStamp::NEUTRAL,
                            });
                        }
                        ObjectMember::CallSignature(func) => {
                            let function_expr = TypeExpr::Function(Arc::new(func.clone()));
                            call_signatures
                                .push(self.lower_locator_shape_node(&function_expr, ctx));
                        }
                        ObjectMember::ConstructSignature(func) => {
                            let function_expr = TypeExpr::Function(Arc::new(func.clone()));
                            construct_signatures
                                .push(self.lower_locator_shape_node(&function_expr, ctx));
                        }
                        ObjectMember::IndexSignature(sig) => {
                            index_signatures.push(IndexSignature {
                                key_type: self.lower_locator_shape_node(&sig.key_type, ctx),
                                value_type: self.lower_locator_shape_node(&sig.value_type, ctx),
                                readonly: sig.readonly,
                                spans: sig.spans,
                                declaration_origin: declaration_origin.clone(),
                            })
                        }
                    }
                }
                let has_index_signature = !index_signatures.is_empty();
                let view = SurfaceView {
                    members: Arc::from(members.into_boxed_slice()),
                    call_signatures: Arc::from(call_signatures.into_boxed_slice()),
                    construct_signatures: Arc::from(construct_signatures.into_boxed_slice()),
                    index_signatures: Arc::from(index_signatures.into_boxed_slice()),
                    keyspace: None,
                    has_index_signature,
                };
                graph.intern_node_with_scope(SemanticNodeData::Object(view), scope.clone())
            }
        }
    }

    /// Lower an argument slice in order into the interned-id slice a
    /// reference / import-type carrier holds.
    fn lower_locator_shape_args(
        &self,
        args: &[TypeExpr],
        ctx: &ShapeLowerCtx<'_>,
    ) -> Arc<[SemanticNodeId]> {
        let lowered: Vec<SemanticNodeId> = args
            .iter()
            .map(|arg| self.lower_locator_shape_node(arg, ctx))
            .collect();
        Arc::from(lowered.into_boxed_slice())
    }

    /// Lower a function / constructor signature. A function's own `<T>`
    /// binders shadow outer generics, each parameter's constraint / default
    /// seeing the PRIOR own binders (TS scoping). An absent return
    /// annotation mirrors the shared `Opaque(Miss)` placeholder.
    fn lower_locator_shape_function(
        &self,
        func: &FunctionExpr,
        ctx: &ShapeLowerCtx<'_>,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let scope = ctx.scope;
        let mut own_frame = LocatorBinderFrame::default();
        let mut type_parameters: Vec<TypeParamDecl> =
            Vec::with_capacity(func.type_parameters.len());
        for tp in &func.type_parameters {
            let display_name: Arc<str> = Arc::from(tp.name.as_str());
            let head_storage;
            let head_ctx = if type_parameters.is_empty() {
                *ctx
            } else {
                let mut frames: Vec<LocatorBinderFrame> = ctx.binders.to_vec();
                frames.push(own_frame.clone());
                head_storage = frames;
                ctx.with_binders(&head_storage)
            };
            let constraint = tp
                .constraint
                .as_deref()
                .map(|c| self.lower_locator_shape_node(c, &head_ctx));
            let default = tp
                .default
                .as_deref()
                .map(|d| self.lower_locator_shape_node(d, &head_ctx));
            let decl = DeclIdentity::from_scope(scope, Arc::clone(&display_name));
            let binder = graph.intern_node_with_scope(
                SemanticNodeData::TypeParam {
                    decl,
                    param_index: 0,
                    constraint,
                    default,
                    display_name: Arc::clone(&display_name),
                },
                scope.clone(),
            );
            own_frame.bind(Arc::clone(&display_name), binder);
            type_parameters.push(TypeParamDecl {
                name: display_name,
                constraint,
                default,
            });
        }

        let inner_storage;
        let inner_ctx = if func.type_parameters.is_empty() {
            *ctx
        } else {
            let mut frames: Vec<LocatorBinderFrame> = ctx.binders.to_vec();
            frames.push(own_frame);
            inner_storage = frames;
            ctx.with_binders(&inner_storage)
        };

        let params: Vec<FunctionParam> = func
            .parameters
            .iter()
            .map(|p| FunctionParam {
                name: p.name.as_deref().map(Arc::<str>::from),
                ty: self.lower_locator_shape_node(&p.ty, &inner_ctx),
                optional: p.optional,
                rest: p.rest,
                span: p.span,
            })
            .collect();
        let return_type = match func.return_type.as_deref() {
            Some(ret) => self.lower_locator_shape_node(ret, &inner_ctx),
            None => graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
        };
        graph.intern_node_with_scope(
            SemanticNodeData::Function {
                params: Arc::from(params.into_boxed_slice()),
                return_type,
                type_parameters: Arc::from(type_parameters.into_boxed_slice()),
                signature_span: func.spans.signature,
                return_type_span: func.spans.return_type,
            },
            scope.clone(),
        )
    }

    /// Resolve a reference head to its IDENTITY carrier — never an
    /// execution:
    ///
    /// 1. a bound binder name returns its shell node (an APPLIED binder has
    ///    no faithful shape — fail closed);
    /// 2. a name the shared bare-name resolver settles interns a `DeclRef`
    ///    (bare) / `InstantiationRef` (applied) identity carrier — userland
    ///    resolution wins over a same-named global lib head;
    /// 3. an unshadowed global lib head (`Promise` / builtin utilities)
    ///    interns the `__builtin__` `InstantiationRef` carrier;
    /// 4. an unresolvable name stays a `BareRef` carrier for the demand
    ///    points.
    fn resolve_locator_ref_head(
        &self,
        name: &Arc<str>,
        type_arguments: &[TypeExpr],
        ctx: &ShapeLowerCtx<'_>,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let scope = ctx.scope;

        if let Some(binder) = ctx.lookup_binder(name) {
            if type_arguments.is_empty() {
                return binder;
            }
            // An applied binder (`T<X>` where `T` is a bound type
            // parameter) has no faithful authored shape — fail closed
            // rather than leak the shadowed name as an unbound reference.
            return self.opaque(QueryError::Miss);
        }

        // A script-setup generic parameter is NOT a declaration: its bare
        // name stays a `BareRef` carrier so the view projection resolves it
        // to the rich `TypeParam` shell from the scope payload's type
        // bindings (mirrors the reducing entry's dedicated arm, which
        // precedes the shared head resolver). Resolving it here would cache
        // a bogus `DeclRef` identity in the shape.
        if ctx
            .scope_payload
            .is_some_and(|payload| payload.scope_type_bindings.contains_key(name.as_ref()))
        {
            return graph.intern_node_with_scope(
                SemanticNodeData::new_bare_ref(
                    Arc::clone(name),
                    scope.clone(),
                    self.lower_locator_shape_args(type_arguments, ctx),
                ),
                scope.clone(),
            );
        }

        // Identity resolution in the AUTHORED declaration's own lexical
        // scope — the declaration's `name_resolution` map first (the same
        // import / namespace-sibling-aware fast path the reducing entry
        // consults, so a declaration-local / namespace / import binding
        // shadowing a top-level symbol resolves to the SHADOWING identity),
        // then the ONE shared payload-aware bare-name resolver. A map hit
        // re-canonicalizes through `resolve_imported_type_root` to the
        // FINAL defining identity (a bundle-fallback entry may carry the
        // intermediate barrel hop — the same final-hop retry
        // `resolve_prepared_type_decl_via_host` applies). Userland
        // shadowing wins over the global lib heads below by resolution
        // order. This resolves WHO the name is (a slot), never WHAT it
        // means (no `ResolveDecl` / `Instantiate` execution).
        let resolved = ctx
            .name_resolution
            .and_then(|map| map.get(name.as_ref()))
            .and_then(|direct| {
                // A map entry whose canonical does not name a live file is
                // not a usable identity — e.g. an empty canonical, or an
                // ambient string-literal module SPECIFIER the bundle
                // canonicalization stored verbatim at prep time. Fall
                // through to the full in-scope resolver, which resolves the
                // specifier through the live dependency/ambient-module
                // authority.
                if direct.canonical_id.is_empty() {
                    return None;
                }
                // Facts-returning form + tracer record: the route-chain
                // facts (every barrel/re-export participant's version) enter
                // the active fact tracer and land in the enclosing
                // `LowerLocator` entry's `ReadSetSignature`, so a barrel
                // retarget with the owner unchanged MISSES the warm shape.
                let ((final_canonical, final_symbol), route_facts) =
                    self.ctx.resolve_imported_type_root_with_facts(
                        &direct.canonical_id,
                        &direct.symbol_name,
                    );
                self.ctx.observe_borrowed_signature(&route_facts);
                self.ctx.shallow_file_state(&final_canonical)?;
                Some(ResolvedRootIdentity::new(final_canonical, final_symbol))
            })
            .or_else(|| match scope {
                NodeScopeId::File { canonical_id, .. } => resolve_bare_name_in_scope(
                    self.ctx,
                    canonical_id.as_ref(),
                    ctx.scope_payload,
                    name.as_ref(),
                ),
                NodeScopeId::Global => None,
            });
        if let Some(ri) = resolved {
            let canonical: Arc<str> = Arc::from(ri.canonical_id.as_str());
            let decl_name: Arc<str> = Arc::from(ri.symbol_name.as_str());
            let whole_hash = self
                .ctx
                .shallow_file_state(canonical.as_ref())
                .map_or(HashValue::default(), |s| s.whole_hash);
            let identity = DeclIdentity {
                canonical_id: canonical,
                whole_hash,
                decl_name,
            };
            if type_arguments.is_empty() {
                return graph
                    .intern_node_with_scope(SemanticNodeData::DeclRef { identity }, scope.clone());
            }
            return graph.intern_node_with_scope(
                SemanticNodeData::InstantiationRef {
                    base: identity,
                    args: self.lower_locator_shape_args(type_arguments, ctx),
                },
                scope.clone(),
            );
        }

        // Unshadowed global lib heads: `Promise` and the builtin utilities
        // intern the nominal `__builtin__` carrier — NEVER an executed
        // `Instantiate`, in any position.
        if self.is_promise_global_name(name.as_ref())
            || verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(
                name.as_ref(),
            )
            .is_some()
        {
            return graph.intern_node_with_scope(
                SemanticNodeData::InstantiationRef {
                    base: DeclIdentity {
                        canonical_id: Arc::from("__builtin__"),
                        whole_hash: HashValue::default(),
                        decl_name: Arc::clone(name),
                    },
                    args: self.lower_locator_shape_args(type_arguments, ctx),
                },
                scope.clone(),
            );
        }

        // Unresolved: keep the deferred BareRef carrier — demand-time
        // carrier-subject normalization owns its resolution.
        graph.intern_node_with_scope(
            SemanticNodeData::new_bare_ref(
                Arc::clone(name),
                scope.clone(),
                self.lower_locator_shape_args(type_arguments, ctx),
            ),
            scope.clone(),
        )
    }

    /// Build the locator-shape binder frame for a declaration's type
    /// parameters: declared parameters stay SHELLS — one `TypeParam` binder
    /// per declared parameter, each constraint / default lowered under the
    /// frame built so far (TS scoping — earlier parameters are visible to
    /// later heads).
    ///
    /// Returns the frame plus the ordered `(name, binder)` bindings.
    /// Binder identity is deterministic (content-addressed interning over
    /// the same scope + owner + parameter list), so the substitution step
    /// that applies `Instantiate` args to a fetched shape re-derives the
    /// SAME binder ids from the prepared declaration's parameter list.
    ///
    /// The binder's declaration identity carries the OWNING symbol name
    /// plus the parameter's declared ordinal — a DISTINCT identity from
    /// the display-name-keyed shells a nested function type's own `<T>`
    /// binders intern, so applying an argument to the declaration's `T`
    /// can never rewrite a shadowing inner `T`.
    pub(super) fn locator_shape_binder_frame(
        &self,
        scope: &NodeScopeId,
        owner_symbol: &Arc<str>,
        type_parameters: &[verter_type_expr::TypeParam],
        name_resolution: Option<&FxHashMap<String, ResolvedRootIdentity>>,
        scope_payload: Option<&DeclarationScopePayload>,
    ) -> (LocatorBinderFrame, Vec<(Arc<str>, SemanticNodeId)>) {
        let mut frame = LocatorBinderFrame::default();
        let mut bindings: Vec<(Arc<str>, SemanticNodeId)> =
            Vec::with_capacity(type_parameters.len());
        for (index, param) in type_parameters.iter().enumerate() {
            let display_name: Arc<str> = Arc::from(param.name.as_str());
            let (constraint, default) = {
                let head_frames = std::slice::from_ref(&frame);
                let head_ctx =
                    LocatorShapeCtx::new(scope, head_frames, name_resolution, scope_payload);
                (
                    param
                        .constraint
                        .as_deref()
                        .map(|c| self.lower_type_expr_for_locator_shape(c, &head_ctx)),
                    param
                        .default
                        .as_deref()
                        .map(|d| self.lower_type_expr_for_locator_shape(d, &head_ctx)),
                )
            };
            let decl = DeclIdentity::from_scope(scope, Arc::clone(owner_symbol));
            let binder = self.graph().intern_node_with_scope(
                SemanticNodeData::TypeParam {
                    decl,
                    param_index: index as u16,
                    constraint,
                    default,
                    display_name: Arc::clone(&display_name),
                },
                scope.clone(),
            );
            frame.bind(Arc::clone(&display_name), binder);
            bindings.push((display_name, binder));
        }
        (frame, bindings)
    }

    /// Cold build for [`SemanticQueryKey::LowerLocator`] — the SESSION
    /// phase of the two-phase worker-purity split.
    ///
    /// The live `whole_hash` is re-sourced through
    /// `ensure_indexed_ready_serve` (exactly as `build_instantiate` does)
    /// and recorded on the read-set via the observed self-root — never
    /// carried in the key (R6). The WORKER phase
    /// ([`crate::decl_body_memo::DeclBodyMemo::deref_locator_body`]) derefs
    /// the locator through the artifact's retained snapshot (lease-only)
    /// and returns owned typed IR; this build graph-lowers that IR into
    /// ROLE-FREE locator-shape nodes with the decl's type parameters bound
    /// as `TypeParam` shells. The `IndexedReady` Arc — and through it the
    /// memo's `SnapshotLease` — is held THROUGH the graph-lowering call
    /// (releasing it before the deref would force a reparse).
    pub(super) fn build_lower_locator(
        &self,
        key: &LocatorLoweringKey,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput {
        let canonical = Arc::clone(&key.slot().defining_canonical);
        let Some(serve) = self.ctx.ensure_indexed_ready_serve(canonical.as_ref()) else {
            // The owning file is unknown to the live view — the value
            // cannot be self-rooted; refuse warm admission.
            let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput =
                (QueryResult::Error(QueryError::Miss), empty_signature()).into();
            output.cache_suppress = true;
            return output;
        };
        // Keeps the ShallowFileState → DeclBodyMemo → SnapshotLease alive
        // through the graph lowering below.
        let indexed = serve.indexed;
        let observed_self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot> =
            vec![(Arc::clone(&canonical), indexed.whole_hash)];

        let derefed = match indexed
            .shallow_state
            .decl_bodies()
            .deref_locator_body(key.locator())
        {
            Ok(derefed) => derefed,
            // Every deref failure is a typed fail-closed non-result; an
            // Error result is never warm-published.
            Err(_) => {
                let output: crate::project_semantic_dispatch::walk::QueryBuildOutput = (
                    QueryResult::Error(QueryError::Miss),
                    self.project_generation_signature(),
                )
                    .into();
                return output.with_observed_self_roots(observed_self_roots);
            }
        };

        let scope = NodeScopeId::File {
            canonical_id: Arc::clone(&canonical),
            whole_hash: indexed.whole_hash,
            local_scope: None,
        };
        let anchor_symbol = match key.locator() {
            AuthoredBodyLocator::DeclBody(slot) => Arc::clone(&slot.anchor.symbol),
            AuthoredBodyLocator::AugmentationBody(aug) => Arc::clone(&aug.anchor.symbol),
            AuthoredBodyLocator::MacroPayload(payload) => Arc::clone(&payload.anchor.symbol),
        };

        // Anchor-scope IDENTITY-resolution inputs — the SAME sources the
        // reducing path threads to its reference-head resolution (the
        // anchor declaration's import / namespace-sibling-aware
        // `name_resolution` map plus the bundle's declaration-scope
        // payload), so the cached `DeclRef` / `InstantiationRef` identities
        // match the reducing path's under declaration-local / namespace /
        // import shadowing. Absence (no prepared declaration for the
        // anchor) degrades to the payload-aware in-scope resolver.
        let bundle = self.ctx.prepared_decl_bundle(canonical.as_ref());
        let scope_payload = bundle
            .as_ref()
            .map(|b| DeclarationScopePayload::from_bundle(b));
        let dep_edges = bundle.as_ref().map(|b| Arc::clone(&b.dep_edges));
        let prepared_anchor: Option<AnchorPreparedDecl> = match key.locator() {
            AuthoredBodyLocator::DeclBody(slot) => match slot.anchor.space {
                verter_type_expr::locators::LocatorSymbolSpace::Type => self
                    .ctx
                    .prepared_type_decl(canonical.as_ref(), anchor_symbol.as_ref())
                    .map(AnchorPreparedDecl::Type),
                verter_type_expr::locators::LocatorSymbolSpace::Value => self
                    .ctx
                    .prepared_value_decl(canonical.as_ref(), anchor_symbol.as_ref())
                    .map(AnchorPreparedDecl::Value),
                verter_type_expr::locators::LocatorSymbolSpace::Namespace => None,
            },
            AuthoredBodyLocator::AugmentationBody(aug) => {
                use verter_semantic::analysis::type_eval::AugmentationScopeKind;
                let scope_kind = match &aug.scope {
                    verter_type_expr::locators::AuthoredAugmentationScope::Global => {
                        AugmentationScopeKind::Global
                    }
                    verter_type_expr::locators::AuthoredAugmentationScope::Module { specifier } => {
                        AugmentationScopeKind::Module(specifier.as_ref().to_string())
                    }
                };
                crate::resolver_core::prepare_augmentation_type_decl(
                    canonical.as_ref(),
                    &indexed.shallow_state,
                    &scope_kind,
                    anchor_symbol.as_ref(),
                    dep_edges.as_deref(),
                )
                .map(|prepared| AnchorPreparedDecl::Augmentation(Box::new(prepared)))
            }
            AuthoredBodyLocator::MacroPayload(_) => None,
        };
        let name_resolution = prepared_anchor
            .as_ref()
            .map(AnchorPreparedDecl::name_resolution);

        let (frame, _bindings) = self.locator_shape_binder_frame(
            &scope,
            &anchor_symbol,
            &derefed.type_parameters,
            name_resolution,
            scope_payload.as_ref(),
        );
        let frames = [frame];
        let shape_ctx =
            LocatorShapeCtx::new(&scope, &frames, name_resolution, scope_payload.as_ref());

        let node = match derefed.shape {
            DerefedBodyShape::Single(expr) => {
                self.lower_type_expr_for_locator_shape(&expr, &shape_ctx)
            }
            // A whole merged decl body lowers each contributor and interns
            // the DISTINCT MergedDecl carrier — never a bare Intersection
            // (the peer-merge reducer needs the contributor structure).
            DerefedBodyShape::Merged(contributors) => {
                let ids: Vec<SemanticNodeId> = contributors
                    .iter()
                    .map(|contributor| {
                        self.lower_type_expr_for_locator_shape(contributor, &shape_ctx)
                    })
                    .collect();
                self.graph().intern_node_with_scope(
                    SemanticNodeData::MergedDecl {
                        contributors: Arc::from(ids.into_boxed_slice()),
                    },
                    scope.clone(),
                )
            }
        };

        let output: crate::project_semantic_dispatch::walk::QueryBuildOutput = (
            QueryResult::Value(node),
            self.project_generation_signature(),
        )
            .into();
        output.with_observed_self_roots(observed_self_roots)
    }

    /// The session-private locator-shape provider: lower the fixed authored
    /// shape of the body `locator` names, memoized under the first-class
    /// [`SemanticQueryKey::LowerLocator`] query on the shared
    /// multi-candidate read-set-validated substrate.
    ///
    /// The env-bearing slot is derived FROM the locator anchor (so the
    /// key's anchor-match gate holds by construction) with the anchor
    /// canonical's LIVE `parse_env_hash` / `resolve_env_hash` dimensions.
    /// Strictly unsubstituted: substituted demands route through
    /// `Instantiate { args }`, never through this provider.
    ///
    /// This is the production body source:
    /// `lower_located_body_with_provenance` demands every declaration body
    /// through this provider (never a prepared-body read), then owns
    /// args-substitution and the post-substitution view projection.
    pub(crate) fn lower_locator(
        &self,
        locator: AuthoredBodyLocator,
    ) -> QueryResult<SemanticNodeId> {
        let anchor = match &locator {
            AuthoredBodyLocator::DeclBody(slot) => &slot.anchor,
            AuthoredBodyLocator::AugmentationBody(aug) => &aug.anchor,
            AuthoredBodyLocator::MacroPayload(payload) => &payload.anchor,
        };
        let slot = self
            .type_slot_for(Arc::clone(&anchor.canonical_id), Arc::clone(&anchor.symbol))
            .with_symbol_space(semantic_space_for_locator_space(anchor.space));
        let env = self
            .ctx
            .host_for_fact_tracer_install()
            .host_view_env_hashes_for(anchor.canonical_id.as_ref());
        let key = match LocatorLoweringKey::new_unsubstituted(
            slot,
            locator,
            ParseEnvHash::from_env_hash(env.parse_env_hash),
            ResolveEnvHash::from_env_hash(env.resolve_env_hash),
        ) {
            Ok(key) => key,
            // Unreachable by construction — the slot is derived from the
            // locator anchor — but a malformed identity must fail closed,
            // never lower under the wrong slot.
            Err(_) => return QueryResult::Error(QueryError::Miss),
        };
        let read = self.execute_read(SemanticQueryKey::LowerLocator { key });
        crate::meta_resolve::emit_dispatch_dep_signature_facts(self.ctx, &read.dep_signature);
        read.value
    }
}
