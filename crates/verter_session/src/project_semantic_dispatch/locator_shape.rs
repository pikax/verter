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
use verter_type_expr::locators::{TypeParamBoundPosition, TypeParamVisibility};

use crate::resolver_core::bare_name_resolve::{
    resolve_bare_name_in_scope, DeclarationScopePayload,
};
use crate::semantic_query::{
    DeclIdentity, FunctionParam, HashValue, IndexKey, IndexSignature, MacroOwnBodyStamp, MapperKey,
    MapperKind, MergeRoleStamp, NodeScopeId, OptionalityMod, PrimitiveKind, QueryError,
    QueryResult, ReadonlyMod, ScopeId, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
    SurfaceEntry, SurfaceMember, SurfaceView, SyntheticBindingId, TupleElement, TypeParamDecl,
    ValueRootKey,
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

fn register_locator_function_alias(
    infer_binders: &crate::semantic_query::InferBinderFactory,
    alias: &TypeExpr,
    original: &FunctionExpr,
) {
    let alias_function = match alias {
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => function,
        _ => return,
    };
    register_locator_function_children_alias(infer_binders, alias_function, original);
}

fn register_locator_function_children_alias(
    infer_binders: &crate::semantic_query::InferBinderFactory,
    alias_function: &FunctionExpr,
    original: &FunctionExpr,
) {
    for (alias_parameter, original_parameter) in alias_function
        .type_parameters
        .iter()
        .zip(&original.type_parameters)
    {
        if let (Some(alias), Some(original)) = (
            alias_parameter.constraint.as_deref(),
            original_parameter.constraint.as_deref(),
        ) {
            infer_binders.register_equivalent_subtree(alias, original);
        }
        if let (Some(alias), Some(original)) = (
            alias_parameter.default.as_deref(),
            original_parameter.default.as_deref(),
        ) {
            infer_binders.register_equivalent_subtree(alias, original);
        }
    }
    for (alias_parameter, original_parameter) in
        alias_function.parameters.iter().zip(&original.parameters)
    {
        infer_binders.register_equivalent_subtree(&alias_parameter.ty, &original_parameter.ty);
    }
    if let (Some(alias), Some(original)) = (
        alias_function.return_type.as_deref(),
        original.return_type.as_deref(),
    ) {
        infer_binders.register_equivalent_subtree(alias, original);
    }
}

fn register_locator_object_member_alias(
    infer_binders: &crate::semantic_query::InferBinderFactory,
    alias: &ObjectMember,
    original: &ObjectMember,
) {
    match (alias, original) {
        (ObjectMember::Property(alias), ObjectMember::Property(original)) => {
            infer_binders.register_equivalent_subtree(&alias.ty, &original.ty);
        }
        (ObjectMember::Spread(alias), ObjectMember::Spread(original)) => {
            infer_binders.register_equivalent_subtree(&alias.ty, &original.ty);
        }
        (ObjectMember::IndexSignature(alias), ObjectMember::IndexSignature(original)) => {
            infer_binders.register_equivalent_subtree(&alias.key_type, &original.key_type);
            infer_binders.register_equivalent_subtree(&alias.value_type, &original.value_type);
        }
        (ObjectMember::Method(alias), ObjectMember::Method(original)) => {
            register_locator_function_children_alias(
                infer_binders,
                &alias.function,
                &original.function,
            );
        }
        (ObjectMember::CallSignature(alias), ObjectMember::CallSignature(original)) => {
            register_locator_function_children_alias(infer_binders, alias, original);
        }
        (ObjectMember::ConstructSignature(alias), ObjectMember::ConstructSignature(original)) => {
            register_locator_function_children_alias(infer_binders, alias, original);
        }
        _ => {}
    }
}

impl AnchorPreparedDecl {
    fn name_resolution(&self) -> &FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity> {
        match self {
            Self::Type(prepared) => &prepared.name_resolution,
            Self::Value(prepared) => &prepared.name_resolution,
            Self::Augmentation(prepared) => &prepared.name_resolution,
        }
    }
}

/// One name slot of a binder frame: whether the name is a referenceable
/// binder or a shadow-only entry.
///
/// A shadow-only entry exists because TS lexical visibility of a type
/// parameter's DEFAULT bound is asymmetric: the parameter itself and later
/// siblings still SHADOW any outer same-named declaration (the name is
/// declared in the frame), but a reference to them is illegal — it must
/// resolve unbound-within-frame, never fall through to the outer symbol.
#[derive(Debug, Clone, Copy)]
pub(crate) enum BinderSlot {
    /// The name is a referenceable binder: a reference binds this node.
    Usable(SemanticNodeId),
    /// The name is declared in this frame (it shadows outer same-named
    /// declarations) but is FORBIDDEN as a reference from the current
    /// position: a reference lowers to the fail-closed `Opaque(Miss)`.
    ShadowOnly,
}

/// One lexical binder frame of the locator-shape lowering: declared
/// type-parameter / `infer` / mapped-binder names in scope at one nesting
/// level, each mapped to its [`BinderSlot`].
#[derive(Debug, Default, Clone)]
pub(crate) struct LocatorBinderFrame {
    names: FxHashMap<Arc<str>, BinderSlot>,
    infer_declarations: FxHashMap<Arc<str>, SemanticNodeId>,
}

impl LocatorBinderFrame {
    /// Bind a syntactic binder name to its interned binder node.
    pub(crate) fn bind(&mut self, name: Arc<str>, node: SemanticNodeId) {
        self.names.insert(name, BinderSlot::Usable(node));
    }

    fn bind_infer_declaration(&mut self, name: Arc<str>, node: SemanticNodeId) {
        self.infer_declarations.insert(name, node);
    }

    /// Declare a syntactic binder name as present-but-forbidden: it shadows
    /// outer same-named declarations without being referenceable.
    fn bind_shadow_only(&mut self, name: Arc<str>) {
        self.names.insert(name, BinderSlot::ShadowOnly);
    }

    fn lookup(&self, name: &str) -> Option<BinderSlot> {
        self.names.get(name).copied()
    }

    fn lookup_infer_declaration(&self, name: &str) -> Option<SemanticNodeId> {
        self.infer_declarations.get(name).copied()
    }
}

/// Identity minting mode for one declared type-parameter binder in the
/// shared binder-frame constructor
/// ([`ProjectSemanticDispatch::build_type_param_binder_frame`]).
pub(super) enum BinderIdentityMode<'a> {
    /// A type-declaration HEADER parameter: binder identity = the OWNING
    /// symbol name + the parameter's declared ordinal.
    DeclHeader {
        /// The declaring symbol's stable merged name.
        owner_symbol: &'a Arc<str>,
    },
    /// A free authored `TypeParameter` occurrence: binder identity = the
    /// display name at ordinal 0.
    Signature,
    /// A FUNCTION / constructor signature's own generic clause: binder
    /// identity = the declaring anchor entity (when the lowering position
    /// carries one) + the parameter's declared clause ordinal, and the
    /// interned binder node carries NO bound content — constraints and
    /// defaults are metadata on the signature's `TypeParamDecl` list, never
    /// part of the binder's interned identity. Every parameter predeclares
    /// its one binder first; constraints then lower under the COMPLETE
    /// binder map (self and forward sibling references bind the true
    /// binder, so an instantiation substitution reaches them), while
    /// defaults keep the prior-sibling-only frame (TS2744: a default's
    /// self / forward reference is shadow-forbidden).
    FunctionSignature {
        /// The declaring anchor entity's stable symbol, when the lowering
        /// position names one — it qualifies the binder identity so two
        /// same-name parameters of DIFFERENT declared functions in one
        /// file never share a binder node.
        anchor_symbol: Option<&'a Arc<str>>,
    },
}

/// Presence spec of one declared type parameter for the shared binder-frame
/// constructor: name + bound presence. The bound BODIES enter through the
/// caller's bound-lowering strategy (typed-IR inline on the shape path, the
/// memoized `LowerLocator` query on the fact path), so both paths intern
/// IDENTICAL binder ids from identical specs.
pub(super) struct TypeParamBinderSpec {
    pub(super) name: Arc<str>,
    pub(super) has_constraint: bool,
    pub(super) has_default: bool,
}

/// One minted type-parameter binder of a declaration-header / signature
/// frame: the final (bound-carrying) binder node plus its lowered bound
/// nodes.
pub(super) struct BuiltTypeParamBinder {
    pub(super) name: Arc<str>,
    pub(super) binder: SemanticNodeId,
    pub(super) constraint: Option<SemanticNodeId>,
    pub(super) default: Option<SemanticNodeId>,
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
    name_resolution: Option<&'a FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity>>,
    /// The anchor scope's declaration-scope payload (scope-local names +
    /// import bindings), threaded to the shared in-scope resolver.
    scope_payload: Option<&'a DeclarationScopePayload>,
    /// Exact content-free authored source anchoring conditional-`infer`
    /// declarations lowered through this locator family.
    infer_source: Option<&'a AuthoredBodyLocator>,
}

impl<'a> LocatorShapeCtx<'a> {
    /// Compose the sealed locator-shape context over the authored
    /// position's lexical `scope`, its type-parameter binder frames, and
    /// the declaration's bare-name resolution inputs.
    pub(crate) fn new(
        scope: &'a NodeScopeId,
        binders: &'a [LocatorBinderFrame],
        name_resolution: Option<&'a FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity>>,
        scope_payload: Option<&'a DeclarationScopePayload>,
    ) -> Self {
        Self {
            scope,
            binders,
            name_resolution,
            scope_payload,
            infer_source: None,
        }
    }

    pub(crate) fn with_infer_source(mut self, source: &'a AuthoredBodyLocator) -> Self {
        self.infer_source = Some(source);
        self
    }

    fn with_optional_infer_source(mut self, source: Option<&'a AuthoredBodyLocator>) -> Self {
        self.infer_source = source;
        self
    }
}

/// The module-internal working context of one locator-shape lowering pass.
#[derive(Clone, Copy)]
struct ShapeLowerCtx<'a> {
    scope: &'a NodeScopeId,
    binders: &'a [LocatorBinderFrame],
    name_resolution: Option<&'a FxHashMap<std::sync::Arc<str>, ResolvedRootIdentity>>,
    scope_payload: Option<&'a DeclarationScopePayload>,
    infer_binders: &'a crate::semantic_query::InferBinderFactory,
    infer_source: Option<&'a AuthoredBodyLocator>,
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
            infer_binders: self.infer_binders,
            infer_source: self.infer_source,
        }
    }

    /// The binder slot `name` stands for, scanning innermost-last outward.
    /// The FIRST frame declaring the name decides (an inner binder shadows
    /// an outer one; a shadow-only entry shadows without being usable).
    fn lookup_binder(&self, name: &str) -> Option<BinderSlot> {
        self.binders
            .iter()
            .rev()
            .find_map(|frame| frame.lookup(name))
    }

    fn lookup_infer_declaration(&self, name: &str) -> Option<SemanticNodeId> {
        self.binders
            .iter()
            .rev()
            .find_map(|frame| frame.lookup_infer_declaration(name))
    }
}

#[path = "locator_shape_binder.rs"]
mod binder;

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
        let infer_binders = match ctx.infer_source {
            Some(source) => crate::semantic_query::InferBinderFactory::for_authored_locator(
                ctx.scope, expr, source,
            ),
            None => crate::semantic_query::InferBinderFactory::new(ctx.scope, expr),
        };
        let work = ShapeLowerCtx {
            scope: ctx.scope,
            binders: ctx.binders,
            name_resolution: ctx.name_resolution,
            scope_payload: ctx.scope_payload,
            infer_binders: &infer_binders,
            infer_source: ctx.infer_source,
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
                    // Locator-shape shell lowering: authored order and
                    // scope, never a reduction.
                    graph.intern_node_with_scope(
                        SemanticNodeData::Union(
                            crate::semantic_query::composite::CompositeList::authored_shell(ids),
                        ),
                        scope.clone(),
                    )
                }
            }
            TypeExpr::Intersection(arms) => {
                let ids = self.lower_locator_shape_args(arms, ctx);
                if ids.is_empty() {
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
                } else if ids.len() == 1 {
                    ids[0]
                } else {
                    // Locator-shape shell lowering: see the Union arm above.
                    graph.intern_node_with_scope(
                        SemanticNodeData::Intersection(
                            crate::semantic_query::composite::CompositeList::authored_shell(ids),
                        ),
                        scope.clone(),
                    )
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
                            None => IndexKey::Computed(self.lower_locator_shape_node(index, ctx)),
                        }
                    }
                    _ => IndexKey::Computed(self.lower_locator_shape_node(index, ctx)),
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
                let extends_path = ctx.infer_binders.path_for_expr(expr).child(
                    crate::semantic_query::infer_binder_names::
                        InferSyntaxPathStep::ConditionalExtends,
                );
                let infer_sites =
                    crate::semantic_query::infer_binder_names::collect_extends_infer_declarations(
                        extends,
                        &extends_path,
                    );
                let mut declaration_frame = LocatorBinderFrame::default();
                let mut declarations = Vec::with_capacity(infer_sites.len());
                for site in infer_sites {
                    let binder = ctx.infer_binders.binder_at(&site.path);
                    let declaration = graph.intern_node_with_scope(
                        SemanticNodeData::Infer {
                            name: Arc::clone(&site.name),
                            binder: binder.clone(),
                        },
                        scope.clone(),
                    );
                    declaration_frame.bind_infer_declaration(Arc::clone(&site.name), declaration);
                    declarations.push((site.name, binder));
                }
                let mut extends_frames: Vec<LocatorBinderFrame> = ctx.binders.to_vec();
                extends_frames.push(declaration_frame);
                let extends_ctx = ctx.with_binders(&extends_frames);
                let extends_id = self.lower_locator_shape_node(extends, &extends_ctx);
                let true_id = if declarations.is_empty() {
                    self.lower_locator_shape_node(true_type, ctx)
                } else {
                    let mut infer_frame = LocatorBinderFrame::default();
                    for (name, binder) in declarations {
                        let reference = graph.intern_node_with_scope(
                            SemanticNodeData::InferRef {
                                name: Arc::clone(&name),
                                binder,
                            },
                            scope.clone(),
                        );
                        infer_frame.bind(name, reference);
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
                let (source_node, key_space, base_infer_name) = match source.as_ref() {
                    TypeExpr::KeyOf(inner) => {
                        let inner_id = self.lower_locator_shape_node(inner, ctx);
                        let key_space = graph.intern_node_with_scope(
                            SemanticNodeData::KeyOf { base: inner_id },
                            scope.clone(),
                        );
                        let base_infer = match graph.node_data(inner_id).as_deref() {
                            Some(SemanticNodeData::Infer { name, binder }) => {
                                Some((Arc::clone(name), binder.clone()))
                            }
                            _ => None,
                        };
                        (inner_id, key_space, base_infer)
                    }
                    _ => {
                        let lowered = self.lower_locator_shape_node(source, ctx);
                        (lowered, lowered, None)
                    }
                };

                // Bind only the exact `Infer` declaration selected by the
                // lowered `keyof` operand. Its scoped `InferRef` is visible to
                // the mapped body, while the mapper frame is pushed last so an
                // equal mapper name shadows it.
                let mut frames: Vec<LocatorBinderFrame> = ctx.binders.to_vec();
                if let Some((base_infer_name, binder)) = base_infer_name {
                    let reference = graph.intern_node_with_scope(
                        SemanticNodeData::InferRef {
                            name: Arc::clone(&base_infer_name),
                            binder,
                        },
                        scope.clone(),
                    );
                    let mut base_infer_frame = LocatorBinderFrame::default();
                    base_infer_frame.bind(base_infer_name, reference);
                    frames.push(base_infer_frame);
                }
                let mut mapper_frame = LocatorBinderFrame::default();
                mapper_frame.bind(Arc::clone(&mapper_display_name), parameter_node);
                frames.push(mapper_frame);
                let body_ctx = ctx.with_binders(&frames);

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

            // -- Call / construct signatures — one `Signature` carrier
            //    whose `kind` preserves the spelling. --
            TypeExpr::Function(func) => self.lower_locator_shape_function(
                func,
                ctx,
                crate::semantic_query::SignatureKind::Call,
            ),
            TypeExpr::ConstructorType(func) => self.lower_locator_shape_function(
                func,
                ctx,
                crate::semantic_query::SignatureKind::Construct,
            ),

            // -- Declared type parameters stay SHELLS --
            TypeExpr::TypeParameter(param) => {
                match ctx.lookup_binder(&param.name) {
                    Some(BinderSlot::Usable(binder)) => return binder,
                    // A shadow-forbidden name (a default's self / forward
                    // sibling) is unbound-within-frame — never the outer
                    // same-named declaration.
                    Some(BinderSlot::ShadowOnly) => return self.opaque(QueryError::Miss),
                    None => {}
                }
                // A free authored parameter declaration mints its binder
                // through the ONE shared frame constructor so its own bounds
                // lower under the TS-exact self frame (an F-bounded
                // constraint's self reference binds the predeclared shell; a
                // default's self reference is shadow-forbidden).
                let spec = [TypeParamBinderSpec {
                    name: Arc::from(param.name.as_str()),
                    has_constraint: param.constraint.is_some(),
                    has_default: param.default.is_some(),
                }];
                let base = LocatorShapeCtx::new(
                    scope,
                    ctx.binders,
                    ctx.name_resolution,
                    ctx.scope_payload,
                )
                .with_optional_infer_source(ctx.infer_source);
                let (_frame, built) = self.build_type_param_binder_frame(
                    &base,
                    BinderIdentityMode::Signature,
                    &spec,
                    TypeParamVisibility::Body,
                    |_, position, bound_ctx| {
                        let bound = match position {
                            TypeParamBoundPosition::Constraint => param.constraint.as_deref(),
                            TypeParamBoundPosition::Default => param.default.as_deref(),
                        }?;
                        let bound_work = ShapeLowerCtx {
                            scope: bound_ctx.scope,
                            binders: bound_ctx.binders,
                            name_resolution: bound_ctx.name_resolution,
                            scope_payload: bound_ctx.scope_payload,
                            infer_binders: ctx.infer_binders,
                            infer_source: ctx.infer_source,
                        };
                        Some(self.lower_locator_shape_node(bound, &bound_work))
                    },
                );
                built[0].binder
            }

            // -- Named reference: identity resolution ONLY --
            TypeExpr::Ref {
                name,
                type_arguments,
            } => self.resolve_locator_ref_head(name, type_arguments, ctx),

            TypeExpr::Infer { name } => match ctx.lookup_infer_declaration(name) {
                Some(declaration) => declaration,
                _ => graph.intern_node_with_scope(
                    SemanticNodeData::Infer {
                        name: Arc::from(name.as_str()),
                        binder: ctx.infer_binders.binder_for_expr(expr),
                    },
                    scope.clone(),
                ),
            },
            // Raw fallback — display/compat carrier, never a control signal.
            TypeExpr::Unknown(value) => graph.intern_node_with_scope(
                SemanticNodeData::RawFallback {
                    value: value.clone(),
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
                    NodeScopeId::File {
                        canonical_id,
                        owner,
                        ..
                    } => ScopeId {
                        canonical_id: Arc::clone(canonical_id),
                        owner: *owner,
                        local_scope: None,
                        binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(*owner),
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
                // Spread-bearing locator shapes preserve source ordering in
                // the same canonical program as every other producer.
                if obj
                    .properties
                    .iter()
                    .any(|m| matches!(m, ObjectMember::Spread(_)))
                {
                    let mut effects = Vec::new();
                    let mut run: Vec<&ObjectMember> = Vec::new();
                    let flush = |run: &mut Vec<&ObjectMember>,
                                 effects: &mut Vec<
                        crate::semantic_query::ObjectConstructionEffect,
                    >| {
                        if run.is_empty() {
                            return;
                        }
                        let run_obj = TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
                            properties: run.iter().map(|member| (*member).clone()).collect(),
                        }));
                        if let TypeExpr::Object(alias_object) = &run_obj {
                            for (alias, original) in alias_object.properties.iter().zip(run.iter())
                            {
                                register_locator_object_member_alias(
                                    ctx.infer_binders,
                                    alias,
                                    original,
                                );
                            }
                        }
                        let node = self.lower_locator_shape_node(&run_obj, ctx);
                        let Some(SemanticNodeData::Object(surface)) =
                            self.graph().node_data(node).as_deref().cloned()
                        else {
                            return;
                        };
                        effects.extend(
                            super::object_spread_program_lowering::direct_effects_from_surface(
                                &surface,
                            ),
                        );
                        run.clear();
                    };
                    for member in &obj.properties {
                        match member {
                            ObjectMember::Spread(spread) => {
                                flush(&mut run, &mut effects);
                                let operand = self.lower_locator_shape_node(&spread.ty, ctx);
                                effects.push(
                                    crate::semantic_query::ObjectConstructionEffect::Spread(
                                        operand,
                                    ),
                                );
                            }
                            other => run.push(other),
                        }
                    }
                    flush(&mut run, &mut effects);
                    return graph.intern_node_with_scope(
                        SemanticNodeData::ObjectSpreadProgram(
                            crate::semantic_query::ObjectSpreadProgram {
                                effects: Arc::from(effects),
                            },
                        ),
                        scope.clone(),
                    );
                }
                let declaration_origin = scope.canonical_file();
                let mut members: Vec<SurfaceMember> = Vec::new();
                let mut call_signatures: Vec<SemanticNodeId> = Vec::new();
                let mut construct_signatures: Vec<SemanticNodeId> = Vec::new();
                let mut index_signatures: Vec<IndexSignature> = Vec::new();
                // Authored interleave, kept so the stored surface's
                // `entries` stream is the source order, never the
                // grouped bucket order.
                let mut ordered_entries: Vec<SurfaceEntry> =
                    Vec::with_capacity(obj.properties.len());
                for (member_ordinal, member) in obj.properties.iter().enumerate() {
                    let member_ordinal = u32::try_from(member_ordinal).unwrap_or(u32::MAX);
                    match member {
                        ObjectMember::Property(prop) => {
                            let member_index = members.len();
                            members.push(SurfaceMember {
                                key: prop.key.clone().map(
                                    |computed| self.lower_locator_shape_node(&computed, ctx),
                                    |identity| identity,
                                ),
                                value: self.lower_locator_shape_node(&prop.ty, ctx),
                                optional: prop.optional,
                                readonly: prop.readonly,
                                method_kind: None,
                                has_implementation_body: false,
                                visibility: prop.visibility,
                                // The locator path materializes DECLARATION
                                // bodies: a member reached through a
                                // variable/declaration deref is `NonLiteral`
                                // regardless of the origin the producer recorded
                                // on the authored literal — freshness never
                                // survives declaration materialization.
                                excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
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
                            });
                            ordered_entries
                                .push(SurfaceEntry::Member(members[member_index].clone()));
                        }
                        ObjectMember::Method(method) => {
                            let function_expr =
                                TypeExpr::Function(Arc::new(method.function.clone()));
                            register_locator_function_alias(
                                ctx.infer_binders,
                                &function_expr,
                                &method.function,
                            );
                            let value = self.lower_locator_shape_node(&function_expr, ctx);
                            let value = self.patch_member_signature_occurrence(
                                value,
                                ctx,
                                member_ordinal,
                                0,
                            );
                            let member_index = members.len();
                            members.push(SurfaceMember {
                                key: method.key.clone().map(
                                    |computed| self.lower_locator_shape_node(&computed, ctx),
                                    |identity| identity,
                                ),
                                value,
                                optional: method.optional,
                                readonly: false,
                                method_kind: Some(method.method_kind),
                                has_implementation_body: method.has_implementation_body,
                                visibility: method.visibility,
                                // Declaration materialization is never a
                                // literal origin (see the Property arm).
                                excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
                                spans: method.spans,
                                declaration_origin: declaration_origin.clone(),
                                declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
                                merge_role: MergeRoleStamp::NEUTRAL,
                            });
                            ordered_entries
                                .push(SurfaceEntry::Member(members[member_index].clone()));
                        }
                        ObjectMember::CallSignature(func) => {
                            let function_expr = TypeExpr::Function(Arc::new(func.clone()));
                            register_locator_function_alias(
                                ctx.infer_binders,
                                &function_expr,
                                func,
                            );
                            let node = self.lower_locator_shape_node(&function_expr, ctx);
                            let bucket_ordinal =
                                u32::try_from(call_signatures.len()).unwrap_or(u32::MAX);
                            let node = self.patch_member_signature_occurrence(
                                node,
                                ctx,
                                member_ordinal,
                                bucket_ordinal,
                            );
                            call_signatures.push(node);
                            ordered_entries.push(SurfaceEntry::CallSignature(node));
                        }
                        ObjectMember::ConstructSignature(func) => {
                            let function_expr = TypeExpr::ConstructorType(Arc::new(func.clone()));
                            register_locator_function_alias(
                                ctx.infer_binders,
                                &function_expr,
                                func,
                            );
                            let node = self.lower_locator_shape_node(&function_expr, ctx);
                            let bucket_ordinal =
                                u32::try_from(construct_signatures.len()).unwrap_or(u32::MAX);
                            let node = self.patch_member_signature_occurrence(
                                node,
                                ctx,
                                member_ordinal,
                                bucket_ordinal,
                            );
                            construct_signatures.push(node);
                            ordered_entries.push(SurfaceEntry::ConstructSignature(node));
                        }
                        ObjectMember::IndexSignature(sig) => {
                            let index_index = index_signatures.len();
                            index_signatures.push(IndexSignature {
                                key_type: self.lower_locator_shape_node(&sig.key_type, ctx),
                                value_type: self.lower_locator_shape_node(&sig.value_type, ctx),
                                readonly: sig.readonly,
                                spans: sig.spans,
                                declaration_origin: declaration_origin.clone(),
                            });
                            ordered_entries.push(SurfaceEntry::IndexSignature(
                                index_signatures[index_index].clone(),
                            ));
                        }
                        // Unreachable by construction: the spread-bearing
                        // check above fails the whole object closed before
                        // this member loop runs.
                        ObjectMember::Spread(_) => {}
                    }
                }
                let has_index_signature = !index_signatures.is_empty();
                let view = SurfaceView::from_entries(ordered_entries, None, has_index_signature);
                graph.intern_node_with_scope(SemanticNodeData::Object(view), scope.clone())
            }
        }
    }

    /// Stamp an object member signature's occurrence from the lowering
    /// source locator: the member is the `member_ordinal`-th authored
    /// member of the anchored declaration; `signature_ordinal` is the
    /// member's position in its call/construct bucket. No-op when the node
    /// is not a signature, its occurrence is already set, or the source is
    /// not a whole-body declaration locator.
    fn patch_member_signature_occurrence(
        &self,
        node: SemanticNodeId,
        ctx: &ShapeLowerCtx<'_>,
        member_ordinal: u32,
        signature_ordinal: u32,
    ) -> SemanticNodeId {
        let slot = match ctx.infer_source {
            Some(AuthoredBodyLocator::DeclBody(slot)) if slot.path.is_empty() => slot,
            _ => return node,
        };
        let Some(data) = self.graph().node_data(node) else {
            return node;
        };
        let mut new_data = (*data).clone();
        let SemanticNodeData::Signature {
            occurrence: occurrence_slot,
            ..
        } = &mut new_data
        else {
            return node;
        };
        if occurrence_slot.is_some() {
            return node;
        }
        *occurrence_slot = Some(crate::semantic_query::SignatureNodeOccurrence {
            function: verter_type_expr::facts::FlowFunctionReturnIdentity {
                anchor: slot.anchor.clone(),
                function_part: verter_type_expr::facts::FunctionPartIdentity::Member {
                    member_path: Arc::from([member_ordinal]),
                },
                overload_ordinal: 0,
            },
            signature_ordinal,
        });
        drop(data);
        self.graph().intern_preserving_scope(node, new_data)
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
    /// binders shadow outer generics; each parameter's constraint / default
    /// lowers under the ONE shared binder-frame constructor's TS-exact
    /// per-position frame (a constraint sees every own sibling, a default
    /// sees prior own siblings with self / later shadow-forbidden). An
    /// absent return annotation mirrors the shared `Opaque(Miss)`
    /// placeholder.
    fn lower_locator_shape_function(
        &self,
        func: &FunctionExpr,
        ctx: &ShapeLowerCtx<'_>,
        kind: crate::semantic_query::SignatureKind,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let scope = ctx.scope;
        let specs: Vec<TypeParamBinderSpec> = func
            .type_parameters
            .iter()
            .map(|tp| TypeParamBinderSpec {
                name: Arc::from(tp.name.as_str()),
                has_constraint: tp.constraint.is_some(),
                has_default: tp.default.is_some(),
            })
            .collect();
        let base = LocatorShapeCtx::new(scope, ctx.binders, ctx.name_resolution, ctx.scope_payload)
            .with_optional_infer_source(ctx.infer_source);
        // The declaring anchor entity qualifies the clause's binder
        // identity so same-name parameters of different declared functions
        // in one file never share a binder node.
        let anchor_symbol = ctx.infer_source.map(|locator| match locator {
            AuthoredBodyLocator::DeclBody(slot) => Arc::clone(&slot.anchor.symbol),
            AuthoredBodyLocator::AugmentationBody(body) => Arc::clone(&body.anchor.symbol),
            AuthoredBodyLocator::JsdocTypedefBody(body) => Arc::clone(&body.anchor.symbol),
            AuthoredBodyLocator::MacroPayload(payload) => Arc::clone(&payload.anchor.symbol),
        });
        let (own_frame, built) = self.build_type_param_binder_frame(
            &base,
            BinderIdentityMode::FunctionSignature {
                anchor_symbol: anchor_symbol.as_ref(),
            },
            &specs,
            TypeParamVisibility::Body,
            |ordinal, position, bound_ctx| {
                let tp = func.type_parameters.get(ordinal as usize)?;
                let bound = match position {
                    TypeParamBoundPosition::Constraint => tp.constraint.as_deref(),
                    TypeParamBoundPosition::Default => tp.default.as_deref(),
                }?;
                let bound_work = ShapeLowerCtx {
                    scope: bound_ctx.scope,
                    binders: bound_ctx.binders,
                    name_resolution: bound_ctx.name_resolution,
                    scope_payload: bound_ctx.scope_payload,
                    infer_binders: ctx.infer_binders,
                    infer_source: ctx.infer_source,
                };
                Some(self.lower_locator_shape_node(bound, &bound_work))
            },
        );
        let type_parameters: Vec<TypeParamDecl> = built
            .iter()
            .zip(func.type_parameters.iter())
            .map(|(binder, tp)| TypeParamDecl {
                name: Arc::clone(&binder.name),
                param: binder.binder,
                constraint: binder.constraint,
                default: binder.default,
                is_const: tp.is_const,
            })
            .collect();

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
        let (return_type, return_carrier) = match &func.flow_return {
            // A body-derived return is demanded from the whole-function
            // producer through the sealed helper: the extractor marked the
            // served position with the declaration name; canonical / owner
            // fill from THIS lowering scope (the defining file's). The
            // carrier records the SAME served position.
            Some(identity) => {
                let mut identity = identity.as_ref().clone();
                let scope_canonical = match ctx.scope {
                    NodeScopeId::File {
                        canonical_id,
                        owner,
                        ..
                    } => {
                        identity.anchor.canonical_id = Arc::clone(canonical_id);
                        identity.anchor.owner = *owner;
                        Some(Arc::clone(canonical_id))
                    }
                    _ => None,
                };
                match scope_canonical {
                    Some(scope_canonical) => {
                        let carrier = crate::semantic_query::SignatureReturnCarrier::Function(
                            verter_type_expr::facts::FunctionReturnSource::Flow(identity.clone()),
                        );
                        let return_type = match self.execute_function_return_source(
                            &verter_type_expr::facts::FunctionReturnSource::Flow(identity),
                            scope_canonical.as_ref(),
                        ) {
                            super::flow_return::FunctionReturnNode::Flow(result) => {
                                result.return_type()
                            }
                            _ => graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                        };
                        (return_type, carrier)
                    }
                    None => (
                        graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                        crate::semantic_query::SignatureReturnCarrier::Function(
                            verter_type_expr::facts::FunctionReturnSource::Absent,
                        ),
                    ),
                }
            }
            None => match func.return_type.as_deref() {
                Some(ret) => {
                    let return_type = self.lower_locator_shape_node(ret, &inner_ctx);
                    (
                        return_type,
                        crate::semantic_query::SignatureReturnCarrier::Declared(return_type),
                    )
                }
                None => (
                    graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss)),
                    crate::semantic_query::SignatureReturnCarrier::Function(
                        verter_type_expr::facts::FunctionReturnSource::Absent,
                    ),
                ),
            },
        };
        graph.intern_node_with_scope(
            SemanticNodeData::Signature {
                kind,
                params: Arc::from(params.into_boxed_slice()),
                return_type,
                type_parameters: Arc::from(type_parameters.into_boxed_slice()),
                // The `LowerLocator` producer patches the ROOT signature's
                // occurrence from the key's locator; nested function nodes
                // carry none.
                occurrence: None,
                return_carrier,
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
        let scope = ctx.scope;

        match ctx.lookup_binder(name) {
            Some(BinderSlot::Usable(binder)) => {
                if type_arguments.is_empty() {
                    return binder;
                }
                // An applied binder (`T<X>` where `T` is a bound type
                // parameter) has no faithful authored shape — fail closed
                // rather than leak the shadowed name as an unbound reference.
                return self.opaque(QueryError::Miss);
            }
            // A shadow-forbidden name (a default bound's self / forward
            // sibling): the name is declared in the frame, so it shadows
            // any outer same-named declaration, but the reference itself is
            // illegal — unbound-within-frame, never the outer symbol.
            Some(BinderSlot::ShadowOnly) => return self.opaque(QueryError::Miss),
            None => {}
        }

        // A script-setup generic parameter is NOT a declaration: its bare
        // name stays a `BareRef` carrier so the view projection resolves it
        // to the rich `TypeParam` shell from the scope payload's type
        // bindings (mirrors the reducing entry's dedicated arm, which
        // precedes the shared head resolver). Resolving it here would cache
        // a bogus `DeclRef` identity in the shape.
        if ctx
            .scope_payload
            .is_some_and(|payload| payload.scope_type_bindings().contains_key(name.as_ref()))
        {
            return self.intern_ref_head_carrier(
                crate::project_semantic_dispatch::carrier::RefHeadResolution::Unresolved,
                name,
                scope,
                self.lower_locator_shape_args(type_arguments, ctx),
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
        let resolved: Option<(ResolvedRootIdentity, HashValue)> = ctx
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
                let final_identity =
                    if direct.owner == verter_type_expr::TopLevelOwnerId::ordinary_file() {
                        let (routed, route_facts) = self.ctx.resolve_imported_type_root_with_facts(
                            &direct.canonical_id,
                            &direct.symbol_name,
                        );
                        self.ctx.observe_borrowed_signature(&route_facts);
                        routed.unwrap_or_else(|| direct.clone())
                    } else {
                        direct.clone()
                    };
                // ONE leaf shallow-state retrieval: this read is BOTH the
                // liveness gate (the permitted first canonical shallow
                // materialization — at most once per content generation)
                // AND the versioned-identity hash source. It builds the
                // versioned `DeclIdentity` + correct invalidation facts;
                // it never lowers the target's declaration bodies.
                let state = self.ctx.shallow_file_state(&final_identity.canonical_id)?;
                Some((final_identity, state.whole_hash))
            })
            .or_else(|| match scope {
                NodeScopeId::File { canonical_id, .. } => {
                    let ri = resolve_bare_name_in_scope(
                        self.ctx,
                        canonical_id.as_ref(),
                        scope
                            .top_level_owner()
                            .expect("file scope carries an authored owner"),
                        ctx.scope_payload,
                        name.as_ref(),
                    )?;
                    // Same single retrieval on the bare-name path: one
                    // state read supplies the versioned-identity hash (an
                    // unknown file keeps the default hash — the identity
                    // is still deterministic within the generation).
                    let whole_hash = self
                        .ctx
                        .shallow_file_state(ri.canonical_id.as_ref())
                        .map_or(HashValue::default(), |s| s.whole_hash);
                    Some((ri, whole_hash))
                }
                NodeScopeId::Global => None,
            });
        use crate::project_semantic_dispatch::carrier::RefHeadResolution;
        if let Some((ri, whole_hash)) = resolved {
            let identity = DeclIdentity {
                canonical_id: Arc::clone(&ri.canonical_id),
                owner: ri.owner,
                whole_hash,
                decl_name: Arc::clone(&ri.symbol_name),
            };
            return self.intern_ref_head_carrier(
                RefHeadResolution::Resolved(identity),
                name,
                scope,
                self.lower_locator_shape_args(type_arguments, ctx),
            );
        }

        // Unshadowed global lib heads: `Promise` and the builtin utilities
        // intern the nominal `__builtin__` carrier — NEVER an executed
        // `Instantiate`, in any position.
        if self.runtime_nominal_global_name(name.as_ref()).is_some()
            || verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(
                name.as_ref(),
            )
            .is_some()
        {
            return self.intern_ref_head_carrier(
                RefHeadResolution::Builtin(DeclIdentity {
                    canonical_id: Arc::from("__builtin__"),
                    owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    whole_hash: HashValue::default(),
                    decl_name: Arc::clone(name),
                }),
                name,
                scope,
                self.lower_locator_shape_args(type_arguments, ctx),
            );
        }

        // Unresolved: keep the deferred BareRef carrier — demand-time
        // carrier-subject normalization owns its resolution.
        self.intern_ref_head_carrier(
            RefHeadResolution::Unresolved,
            name,
            scope,
            self.lower_locator_shape_args(type_arguments, ctx),
        )
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
            AuthoredBodyLocator::JsdocTypedefBody(typedef) => &typedef.anchor,
            AuthoredBodyLocator::MacroPayload(payload) => &payload.anchor,
        };
        let slot = self
            .type_slot_for(
                Arc::clone(&anchor.canonical_id),
                anchor.owner,
                Arc::clone(&anchor.symbol),
            )
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
