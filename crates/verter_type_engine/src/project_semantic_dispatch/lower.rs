//! `shallow_lower_type_expr` — TypeExpr → SemanticNodeId shallow lowering
//!
//! Produces the first structural layer of the semantic graph from a
//! parsed [`TypeExpr`] tree. Deeper expansion is the caller's
//! responsibility via [`SemanticQueryKey::ProjectPath`] sub-queries —
//! this pass stays one member / arm / sub-expression deep so the
//! published shell identity is stable across entry paths.
//!
//! **Authority contract:** this is the *only* EAGER (resolving) TypeExpr
//! lowering path in the workspace.
//! `ProjectSemanticDispatch::shallow_lower_type_expr_with_context` below is
//! its one definition. The static views reach it through the
//! `FlowDemandDriver` trait (`flow_return.rs`), whose impls forward to this
//! method, so deleting or renaming it is a compile error at every impl.
//! What a spelling counter cannot see is guarded by the production-source
//! rule `type_expr_lowering_has_exactly_two_single_definition_producers`: no
//! bare-`mode` wrapper beside this producer (every caller states its full
//! [`ProjectionReductionContext`] demand explicitly) and exactly one
//! query-free structural producer `fn lower_type_expr_structural`.

use std::sync::Arc;
use verter_type_expr::OwnedStr;

use rustc_hash::FxHashMap;
use verter_session_query::type_solver::host::ResolvedRootIdentity;
use verter_type_expr::facts::LeafTypeFact;
use verter_type_expr::{FunctionExpr, ObjectMember, TypeExpr};

use super::build::EffectivePreparedValueDecl;
use super::{map_primitive_name, ProjectSemanticDispatch};
use crate::resolver_core::bare_name_resolve::{
    resolve_bare_name_in_scope, DeclarationScopePayload,
};
use crate::resolver_core::scope_shadowing::ScopeShadowing;
use crate::semantic_query::{
    DeclIdentity, HashValue, IndexSignature, NodeScopeId, PrimitiveKind, ProjectionMode,
    ProjectionReductionContext, QueryError, QueryResult, SemanticNodeData, SemanticNodeId,
    SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput, SurfaceEntry, SurfaceMember,
    SurfaceView, TupleElement, ValueRootKey,
};

/// The query boundary whose exact answer a lowering position consumes.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LowerReadShape {
    Node,
    Whole,
}

#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub struct TypeLoweringTrace {
    pub node: SemanticNodeId,
    pub requested: Vec<(SemanticQueryKey, LowerReadShape)>,
    pub substitutions: Vec<(Arc<str>, SemanticNodeId)>,
    pub first_advance_done: bool,
}

/// The lowering context the explicit-stack positions of
/// [`ProjectSemanticDispatch::lower_type_expr_with_infer_factory`] share
/// with their children: everything but the reduction context, which each
/// position derives for its own children.
#[derive(Clone, Copy)]
pub(super) struct TypeExprLowering<'a> {
    pub(super) env: &'a FxHashMap<String, SemanticNodeId>,
    pub(super) scope: &'a NodeScopeId,
    pub(super) name_resolution: &'a FxHashMap<verter_type_expr::OwnedStr, ResolvedRootIdentity>,
    pub(super) scope_payload: Option<&'a DeclarationScopePayload>,
    pub(super) shadowing: &'a ScopeShadowing,
}

impl TypeExprLowering<'_> {
    /// Whether a reference lowers through its head's resolution: it
    /// neither substitutes a parameter bound in the environment nor names
    /// a script-setup generic parameter (both leaves).
    fn resolves_through_head(&self, name: &str, type_arguments: &[TypeExpr]) -> bool {
        !type_arguments.is_empty()
            || !(self.env.contains_key(name)
                || self
                    .scope_payload
                    .is_some_and(|payload| payload.scope_type_bindings().contains_key(name)))
    }
}

/// One step of the explicit-stack lowering: a finished node's value, or
/// the child to lower next (the node itself waits on the stack).
enum LowerStep<'e> {
    Value(SemanticNodeId),
    Descend(&'e TypeExpr, ProjectionReductionContext),
    /// Lend a binding frame's environment until this child finishes. The
    /// cursor restores its previous environment before resuming the parent.
    Scoped(
        &'e TypeExpr,
        ProjectionReductionContext,
        Arc<FxHashMap<String, SemanticNodeId>>,
    ),
    Read(LowerRead),
}

/// A node of the explicit-stack lowering waiting for a child's value:
/// what remains of its own lowering.
enum LowerFrame<'e> {
    Function(FunctionLowering<'e>),
    Mapped(MappedLowering<'e>, MappedAwait),
    Conditional(ConditionalLowering<'e>, ConditionalAwait),
    ImportTail(super::carrier::QualifiedTail),
    Environment(Option<Arc<FxHashMap<String, SemanticNodeId>>>),
    Read(LowerReadResume<'e>),
    TypeofArguments {
        base: SemanticNodeId,
        arguments: &'e [TypeExpr],
        context: ProjectionReductionContext,
        lowered: Vec<SemanticNodeId>,
    },
    /// A named reference whose resolved head consumes its arguments: the
    /// resolution's owned second half, finished once every argument is
    /// lowered.
    RefArguments {
        continuation: crate::project_semantic_dispatch::carrier::CarrierArgsContinuation,
        arguments: &'e [TypeExpr],
        context: ProjectionReductionContext,
        lowered: Vec<SemanticNodeId>,
    },
    /// A union (`union`) or intersection's arms.
    Composite {
        arms: &'e [TypeExpr],
        union: bool,
        context: ProjectionReductionContext,
        lowered: Vec<SemanticNodeId>,
    },
    Array {
        readonly: bool,
    },
    Tuple {
        elements: &'e [verter_type_expr::TupleElement],
        readonly: bool,
        context: ProjectionReductionContext,
        lowered: Vec<TupleElement>,
    },
    Template {
        quasis: &'e [String],
        expressions: &'e [TypeExpr],
        context: ProjectionReductionContext,
        lowered: Vec<SemanticNodeId>,
    },
    KeyOf {
        context: ProjectionReductionContext,
    },
    /// An indexed access whose object is being lowered.
    IndexedObject {
        index: &'e TypeExpr,
        context: ProjectionReductionContext,
    },
    /// An indexed access whose computed index is being lowered.
    IndexedIndex {
        object: SemanticNodeId,
        context: ProjectionReductionContext,
    },
    /// An object type in its member loop at member `next`.
    Object {
        object: &'e verter_type_expr::ObjectExpr,
        next: usize,
        entries: Vec<SurfaceEntry>,
        context: ProjectionReductionContext,
        awaiting: ObjectMemberAwait<'e>,
    },
}

// A direct child and its environment restoration fit without allocating a
// stack buffer. Deeper syntax spills onto the same explicit stack.
type LowerFrames<'e> = smallvec::SmallVec<[LowerFrame<'e>; 2]>;

enum ConditionalAwait {
    Check,
    Constraint,
    Extends,
}
struct ConditionalLowering<'e> {
    expr: &'e TypeExpr,
    context: ProjectionReductionContext,
    check: Option<SemanticNodeId>,
    sites: Vec<crate::semantic_query::infer_binder_names::InferDeclarationSite<'e>>,
    next: usize,
    declarations: Vec<(Arc<str>, crate::semantic_query::InferBinderId)>,
    pending_binder: Option<crate::semantic_query::InferBinderId>,
    extends_env: Option<Arc<FxHashMap<String, SemanticNodeId>>>,
    out_of_scope: Option<Arc<FxHashMap<String, SemanticNodeId>>>,
}
impl<'e> ConditionalLowering<'e> {
    fn new(expr: &'e TypeExpr, context: ProjectionReductionContext) -> Self {
        Self {
            expr,
            context,
            check: None,
            sites: Vec::new(),
            next: 0,
            declarations: Vec::new(),
            pending_binder: None,
            extends_env: None,
            out_of_scope: None,
        }
    }
}

enum MappedAwait {
    Source,
    Value,
    Name,
}
struct MappedLowering<'e> {
    expr: &'e TypeExpr,
    context: ProjectionReductionContext,
    parameter_id: SemanticNodeId,
    over_type_variable: bool,
    source: Option<SemanticNodeId>,
    key_space: Option<SemanticNodeId>,
    value: Option<SemanticNodeId>,
    environment: Option<Arc<FxHashMap<String, SemanticNodeId>>>,
}

enum FunctionStage {
    // Initial bounds see earlier binders (and shells for later siblings in
    // constraints). Published bounds see the completed signature environment.
    BinderConstraint(usize),
    BinderDefault(usize),
    Binder(usize),
    Parameter(usize),
    Return,
    PublishedConstraint(usize),
    PublishedDefault(usize),
    Published(usize),
    Predicate,
    Finish,
}
enum FunctionAwait {
    Constraint,
    Default,
    Parameter(usize),
    Return,
    Predicate,
}
struct FunctionLowering<'e> {
    func: &'e FunctionExpr,
    kind: crate::semantic_query::SignatureKind,
    context: ProjectionReductionContext,
    // None keeps the lent environment without cloning or allocating.
    scoped: Option<Arc<FxHashMap<String, SemanticNodeId>>>,
    clause: Option<Arc<str>>,
    shells: Vec<SemanticNodeId>,
    params: Vec<crate::semantic_query::FunctionParam>,
    type_parameters: Vec<crate::semantic_query::TypeParamDecl>,
    constraint: Option<SemanticNodeId>,
    default: Option<SemanticNodeId>,
    return_type: Option<SemanticNodeId>,
    return_carrier: Option<crate::semantic_query::SignatureReturnCarrier>,
    inferred_predicate: Option<crate::semantic_query::SignaturePredicate>,
    predicate_target: Option<SemanticNodeId>,
    stage: FunctionStage,
    awaiting: FunctionAwait,
}
impl FunctionLowering<'_> {
    fn binder_identity(&self, scope: &NodeScopeId, ordinal: usize) -> (DeclIdentity, u16) {
        match &self.clause {
            Some(clause) => (
                DeclIdentity::from_scope(scope, OwnedStr::from(Arc::clone(clause))),
                u16::try_from(ordinal).unwrap_or(u16::MAX),
            ),
            None => (
                DeclIdentity::from_scope(
                    scope,
                    OwnedStr::from(self.func.type_parameters[ordinal].name.as_str()),
                ),
                0,
            ),
        }
    }
}

/// A lowering's position and completed structural children.
struct TypeLowerCursor<'e> {
    frames: LowerFrames<'e>,
    next: Option<(&'e TypeExpr, ProjectionReductionContext)>,
    environment: Option<Arc<FxHashMap<String, SemanticNodeId>>>,
}

impl<'e> TypeLowerCursor<'e> {
    fn new(expr: &'e TypeExpr, context: ProjectionReductionContext) -> Self {
        Self {
            frames: LowerFrames::new(),
            next: Some((expr, context)),
            environment: None,
        }
    }
}

/// A type's lowering held between reads. The factory's path cache and root
/// identity live for the entire lowering, including every suspension.
pub(super) struct TypeLowerRead<'e> {
    cursor: TypeLowerCursor<'e>,
    infer_binders: crate::semantic_query::InferBinderFactory,
    pub(super) substitutions: Vec<(Arc<str>, SemanticNodeId)>,
}

static_assertions::assert_impl_all!(TypeLowerRead<'_>: Send);
static_assertions::assert_not_impl_any!(TypeLowerRead<'_>: Clone, Copy);

pub(super) struct LowerRead {
    pub(super) key: SemanticQueryKey,
    pub(super) shape: LowerReadShape,
}

/// An answer to the last yielded read, retaining its original envelope.
pub(super) enum LowerDelivery {
    Node(QueryResult<SemanticQueryOutput<SemanticNodeId>>),
    Whole(crate::semantic_query::CacheRead<QueryResult<SemanticNodeId>>),
}

pub(super) enum LowerProgress {
    Done(SemanticNodeId),
    Read(LowerRead),
}

/// The operation waiting for a query answer, with its completed inputs.
enum LowerReadResume<'e> {
    ImportTail,
    ImportAnchor {
        anchor: Box<super::carrier::AnchorRead>,
        arguments: &'e [TypeExpr],
        context: ProjectionReductionContext,
    },
    Anchor {
        anchor: Box<super::carrier::AnchorRead>,
        arguments: &'e [TypeExpr],
        context: ProjectionReductionContext,
    },
    CarrierFinish(super::carrier::CarrierFinishRead),
    KeyOf,
    MappedKeySpace(MappedLowering<'e>),
    Indexed {
        object: SemanticNodeId,
        index: crate::semantic_query::IndexKey,
        origin: crate::semantic_query::IndexedAccessOrigin,
        context: ProjectionReductionContext,
    },
    Typeof {
        arguments: &'e [TypeExpr],
        context: ProjectionReductionContext,
    },
}

/// Owned scope data for a lowering whose caller did not supply a context.
pub(super) struct OwnedTypeLoweringContext {
    pub(super) scope: NodeScopeId,
    pub(super) env: FxHashMap<String, SemanticNodeId>,
    pub(super) name_resolution: FxHashMap<verter_type_expr::OwnedStr, ResolvedRootIdentity>,
    pub(super) scope_payload: Option<DeclarationScopePayload>,
    pub(super) shadowing: ScopeShadowing,
}

impl OwnedTypeLoweringContext {
    pub(super) fn as_lowering(&self) -> TypeExprLowering<'_> {
        TypeExprLowering {
            env: &self.env,
            scope: &self.scope,
            name_resolution: &self.name_resolution,
            scope_payload: self.scope_payload.as_ref(),
            shadowing: &self.shadowing,
        }
    }
}

pub(super) struct OwnerTypeLowerRead<'e> {
    pub(super) context: OwnedTypeLoweringContext,
    pub(super) read: TypeLowerRead<'e>,
}

static_assertions::assert_impl_all!(OwnerTypeLowerRead<'_>: Send);
static_assertions::assert_not_impl_any!(OwnerTypeLowerRead<'_>: Clone, Copy);

/// The object member whose child is being lowered.
enum ObjectMemberAwait<'e> {
    PropertyValue(&'e verter_type_expr::ObjectProperty),
    IndexKey(&'e verter_type_expr::IndexSignature),
    /// The index signature's value, beside its lowered key.
    IndexValue(&'e verter_type_expr::IndexSignature, SemanticNodeId),
}

fn infer_declaration_env_key(name: &str) -> String {
    format!("\0verter:infer-declaration:{name}")
}

fn register_eager_function_alias(
    infer_binders: &crate::semantic_query::InferBinderFactory,
    alias: &TypeExpr,
    original: &FunctionExpr,
) {
    let alias_function = match alias {
        TypeExpr::Function(function) | TypeExpr::ConstructorType(function) => function,
        _ => return,
    };
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
    if let (Some(alias), Some(original)) = (
        alias_function
            .predicate
            .as_deref()
            .and_then(|predicate| predicate.ty.as_deref()),
        original
            .predicate
            .as_deref()
            .and_then(|predicate| predicate.ty.as_deref()),
    ) {
        infer_binders.register_equivalent_subtree(alias, original);
    }
}

/// The leaf-fact → `TypeExpr` projection for a directly-closed
/// [`LeafTypeFact`] source (the trivially-closed inferred-annotation carrier)
/// — a closed-grammar data projection, not a resolver: a bare `Ref` leaf
/// stays a shallow reference the shared dispatch resolves on demand.
pub fn leaf_type_fact_expr(leaf: &LeafTypeFact) -> TypeExpr {
    match leaf {
        LeafTypeFact::Primitive(name) => TypeExpr::Primitive(*name),
        LeafTypeFact::StringLiteral(value) => TypeExpr::string_literal(value.as_str()),
        LeafTypeFact::NumberLiteral(value) => TypeExpr::number_literal(
            value
                .parse::<f64>()
                .expect("LeafTypeFact::NumberLiteral stores the canonical f64 display string"),
        ),
        LeafTypeFact::BooleanLiteral(value) => {
            TypeExpr::Literal(verter_type_expr::LiteralValue::Boolean(*value))
        }
        LeafTypeFact::Ref(name) => TypeExpr::Ref {
            name: Arc::from(name.as_str()),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        },
    }
}

/// Which declaration certifies a member as an authored `unique symbol`, and
/// therefore which declaration the member's nominal identity anchors on.
enum UniqueSymbolMemberCertification {
    /// The value's OWN inline annotation declares the member (an
    /// object-literal annotation member or a class static): the identity
    /// anchors on the declaring VALUE declaration.
    OwnAnnotation,
    /// The value's annotation references a NAMED type whose declaration
    /// certifies the member. tsc unifies `typeof a.K` and `typeof b.K`
    /// through that one named type (`a: I; b: I` both read the interface's
    /// member), so the identity anchors on the TYPE declaration the
    /// annotation resolved to — anchoring on either value would mint two
    /// nominal types where the checker sees one. Carries the resolved
    /// `(canonical, owner, symbol)` of the certifying type declaration.
    NamedType(
        verter_type_expr::OwnedStr,
        verter_type_expr::TopLevelOwnerId,
        verter_type_expr::OwnedStr,
    ),
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> ProjectSemanticDispatch<'a, C> {
    /// The declaring identity a terminal nominal (`unique symbol`) carrier
    /// records, or `None` for any other node. A single O(1) graph read: the
    /// identity was minted once at the carrier's build site from facts that
    /// build already held, so no reader repeats declaration or route
    /// resolution merely to classify a carrier.
    pub(super) fn unique_symbol_identity_for_typeof_node(
        &self,
        node: SemanticNodeId,
    ) -> Option<verter_type_expr::facts::ValueDeclIdentityPart> {
        self.graph()
            .node_data(node)?
            .typeof_nominal_identity()
            .cloned()
    }

    construction_ordinal_seam! {
        /// The WIDENED structural inhabitant of a nominal (`unique symbol`)
        /// carrier — the bare `symbol` primitive — or `None` when `node` carries
        /// no nominal identity.
        ///
        /// A nominal carrier is TERMINAL under carrier resolution: it
        /// self-resolves, because resolving its head would project the
        /// annotation down to the shared `symbol` primitive and erase the
        /// declaring identity that IS the type. Every consumer that asks a
        /// NOMINAL question (which declaration, which relation) reads the
        /// carrier directly and must never widen.
        ///
        /// A consumer that asks a STRUCTURAL question — which runtime
        /// constructor a prop emits, which callable role a member plays, which
        /// member surface a path hop reads — has no nominal question to ask, and
        /// its `resolved == node` rail means "this reference did not resolve".
        /// Reading a terminal carrier through that rail turns a program that
        /// resolves fine into a missing dependency: a `unique symbol`-typed prop
        /// would lose its `Symbol` runtime constructor and a `unique
        /// symbol`-typed member would publish as an unresolved role. Those
        /// consumers widen through THIS one helper instead, so the structural
        /// answer stays the one the widened primitive always gave.
        fn widened_nominal_typeof(&self, node: SemanticNodeId) -> Option<SemanticNodeId> {
            self.graph()
                .node_data(node)?
                .typeof_nominal_identity()
                .is_some()
                .then(|| {
                    self.graph().intern_node(SemanticNodeData::Primitive(
                        crate::semantic_query::PrimitiveKind::Symbol,
                    ))
                })
        }
    }

    construction_ordinal_seam! {
        /// The same widen applied at EVERY position of `node`'s subtree whose
        /// nominal carrier is declared in `canonical` — the render-boundary
        /// fail-close for a generated declaration surface that cannot name the
        /// owner's own script-setup bindings.
        ///
        /// A nominal carrier renders its authored head (`typeof K`), and a
        /// setup binding is not importable, so a surface that splices that text
        /// would name an identifier it never declares. The ROOT position is not
        /// the only one that can hold such a carrier: `{ inner: typeof K }`, a
        /// union arm, and a signature return all reach the same render. Every
        /// one of them widens to the `symbol` primitive — the honest structural
        /// inhabitant, with no phantom reference and no fresh nominal identity —
        /// through the one substitution route, so a carrier declared in ANOTHER
        /// module keeps its precise spelling wherever it sits.
        ///
        /// The carriers come from the SAME walk that decides which reference
        /// names a rendered surface must have in scope, so a carrier this misses
        /// is one that walk cannot name either.
        fn widen_owner_local_nominal_typeofs(
            &self,
            node: SemanticNodeId,
            canonical: &str,
        ) -> SemanticNodeId {
            let mut carriers: Vec<SemanticNodeId> = Vec::new();
            super::reference_carriers::collect_owner_local_nominal_carriers(
                self,
                node,
                canonical,
                &mut carriers,
            );
            if carriers.is_empty() {
                return node;
            }
            let widened = self.graph().intern_node(SemanticNodeData::Primitive(
                crate::semantic_query::PrimitiveKind::Symbol,
            ));
            let mut result = node;
            for carrier in carriers {
                result = self.substitute_semantic_type_param(result, carrier, widened);
            }
            result
        }
    }

    /// Intern the terminal nominal `typeof` carrier for one declaration.
    ///
    /// The carrier's HEAD stays the AUTHORED reference — the value root and
    /// dotted path the source actually wrote. Nominal equality is decided by
    /// the `identity` PAYLOAD (the declaring `ValueDeclIdentityPart`), so
    /// canonicalising the head as well would buy nothing and would cost the
    /// authored spelling every display, provenance, and locator consumer
    /// reads off the head: `import { TOKEN as T }` would then publish and
    /// display `typeof TOKEN`, naming a symbol that is not in scope in the
    /// consuming file. Two references to one declaration therefore intern as
    /// two nodes that the nominal relation answers EQUAL — which is the
    /// question the relation exists to answer.
    pub(super) fn intern_nominal_typeof(
        &self,
        value_root: ValueRootKey,
        path: verter_type_expr::OwnedSlice<verter_type_expr::OwnedStr>,
        identity: verter_type_expr::facts::ValueDeclIdentityPart,
        scope: NodeScopeId,
    ) -> SemanticNodeId {
        self.graph().intern_node_with_scope(
            SemanticNodeData::new_nominal_typeof(value_root, path, identity),
            scope,
        )
    }

    /// The member-level nominal carrier for a member-qualified
    /// `typeof Root.Member` — the ONE rail a member reference reaches its
    /// declaring identity through, shared by every typeof resolution arm
    /// (the evaluator, the path walker, the raising reducer) so no arm
    /// projects past a nominal member the carrier exists to preserve.
    ///
    /// Qualifies exactly when a certification exists for the member as an
    /// authored `unique symbol` (the `unique_symbol_members` declaration
    /// fact): the carrier's identity is the declaring anchor plus the
    /// member path, so `typeof Tokens.A` and `typeof Tokens.B` are two
    /// nominal types while an inherited `typeof Derived.A` still names the
    /// class that DECLARES `A` (the heritage chain is chased to the
    /// declaring class, and stops at any class that re-declares the member
    /// itself). A member certified through a NAMED type annotation
    /// (`a: I` where `I` declares the member) anchors on the TYPE
    /// declaration, so `typeof a.K` and `typeof b.K` name ONE nominal type
    /// through it. Every other member shape — a non-unique
    /// annotation, a computed key, a deeper projection — answers `None`
    /// and projects/widens exactly as a non-nominal member always has.
    /// Single segment only: a deeper projection reads members of the
    /// `Symbol` interface, which is the widened primitive's surface.
    ///
    /// `root` is the caller's ALREADY-RESOLVED effective value declaration
    /// for `value_root` — the same bare-name resolution plus export-target
    /// walk the caller performed to reach this point. Taking it as an
    /// argument keeps the member rail free of a second identical
    /// bare-name resolve + prepared-decl walk of the root on the cold path
    /// of every member-qualified `typeof`, including the common case where
    /// the member is not a `unique symbol` and the answer is `None`. The
    /// heritage chase below still resolves each BASE it visits, which the
    /// caller has not seen.
    pub(crate) fn member_nominal_typeof(
        &self,
        value_root: &ValueRootKey,
        segments: &[verter_type_expr::OwnedStr],
        root: &EffectivePreparedValueDecl,
        authored_scope: Option<NodeScopeId>,
    ) -> Option<SemanticNodeId> {
        let [segment] = segments else {
            return None;
        };
        // The BFS frontier holds only the bases still to resolve; the root
        // arrives resolved. `visited` is keyed on the EFFECTIVE identity so
        // two spellings that walk to one declaration are visited once.
        let mut worklist: Vec<(
            verter_type_expr::OwnedStr,
            verter_type_expr::TopLevelOwnerId,
            verter_type_expr::OwnedStr,
        )> = Vec::new();
        let mut resolved = Some(root.clone());
        let mut visited = rustc_hash::FxHashSet::default();
        loop {
            let (eff_canonical, eff_owner, eff_symbol, prepared) = match resolved.take() {
                Some(entry) => entry,
                None => {
                    let Some((canonical, owner, symbol)) = worklist.pop() else {
                        break;
                    };
                    match self.effective_prepared_value_decl(
                        canonical.as_ref(),
                        owner,
                        symbol.as_ref(),
                    ) {
                        Some(entry) => entry,
                        None => continue,
                    }
                }
            };
            if !visited.insert((eff_canonical.clone(), eff_owner, eff_symbol.clone())) {
                continue;
            }
            // WHICH declaration the identity anchors on is part of the
            // certification, not a constant: the declaration the BFS is
            // currently visiting is the anchor only for an OWN inline
            // annotation member or a class static. A member certified
            // through a NAMED type anchors on that type's declaration —
            // every value carrying the same annotation names ONE nominal
            // type through it, exactly as the checker unifies them.
            if let Some((id_canonical, id_owner, id_symbol)) =
                match self.unique_symbol_member_certification(&prepared, segment.as_ref()) {
                    Some(UniqueSymbolMemberCertification::OwnAnnotation) => {
                        Some((eff_canonical.clone(), eff_owner, eff_symbol.clone()))
                    }
                    Some(UniqueSymbolMemberCertification::NamedType(canonical, owner, symbol)) => {
                        Some((canonical, owner, symbol))
                    }
                    None => None,
                }
            {
                let identity = verter_type_expr::facts::ValueDeclIdentityPart {
                    canonical_id: id_canonical,
                    owner: id_owner,
                    symbol: id_symbol,
                    member_path: verter_type_expr::OwnedSlice::from(
                        vec![segment.to_string()].into_boxed_slice(),
                    ),
                };
                return Some(match authored_scope {
                    Some(scope) => self.intern_nominal_typeof(
                        value_root.clone(),
                        verter_type_expr::OwnedSlice::from(segments.to_vec().into_boxed_slice()),
                        identity,
                        scope,
                    ),
                    // No authored scope to anchor the carrier under — intern
                    // scope-free; the head still carries the authored
                    // reference every display consumer reads.
                    None => self
                        .graph()
                        .intern_node(SemanticNodeData::new_nominal_typeof(
                            value_root.clone(),
                            verter_type_expr::OwnedSlice::from(
                                segments.to_vec().into_boxed_slice(),
                            ),
                            identity,
                        )),
                });
            }
            // A class that DECLARES the member itself — any annotation at
            // all — is the member's declaring site for this reference: an
            // own declaration shadows every base's member of the same name,
            // so the heritage chase stops here. Without this stop a derived
            // class re-declaring the member with a NON-nominal type would
            // inherit the base's nominal identity and the relation would
            // prove the override's actual type disjoint from itself.
            if self.value_decl_declares_own_member(&prepared, segment.as_ref()) {
                return None;
            }
            for (base_canonical, base_owner, base_name, _args) in
                self.class_heritage_bases(eff_canonical.as_ref(), eff_owner, eff_symbol.as_ref())
            {
                worklist.push((base_canonical, base_owner, base_name));
            }
        }
        None
    }

    /// Which declaration certifies a member as an authored `unique symbol`
    /// — and therefore which declaration the member's nominal identity
    /// anchors on.
    fn unique_symbol_member_certification(
        &self,
        prepared: &verter_session_query::type_solver::PreparedValueDecl,
        member: &str,
    ) -> Option<UniqueSymbolMemberCertification> {
        if prepared
            .type_annotation
            .unique_symbol_members
            .iter()
            .any(|name| name == member)
        {
            return Some(UniqueSymbolMemberCertification::OwnAnnotation);
        }
        let alias_name = match &prepared.type_annotation.reference_head {
            verter_type_expr::facts::AuthoredReferenceHeadFact::Bare { local_name, .. } => {
                local_name.as_ref()
            }
            verter_type_expr::facts::AuthoredReferenceHeadFact::Qualified {
                local_root,
                member_path,
                ..
            } if member_path.is_empty() => local_root.as_ref(),
            _ => return None,
        };
        let root = prepared.name_resolution.get(alias_name)?;
        let head = self.ctx.prepared_type_decl_return_only(
            root.canonical_id.as_ref(),
            root.owner,
            root.symbol_name.as_ref(),
        )?;
        if head.unique_symbol_members.iter().any(|name| name == member) {
            return Some(UniqueSymbolMemberCertification::NamedType(
                root.canonical_id.clone(),
                root.owner,
                root.symbol_name.clone(),
            ));
        }
        let key = verter_type_expr::TypePropertyKey::String(OwnedStr::from(member));
        if head.member_index.contains_key(&key)
            || !self.is_global_type_declaration(
                root.canonical_id.as_ref(),
                root.canonical_id.as_ref(),
                root.owner,
                root.symbol_name.as_ref(),
            )
        {
            return None;
        }
        // A named global interface can declare this member in another file
        // (SymbolConstructor's well-known symbols do). Certify from the
        // declaring contributor's facts, retaining its nominal identity and
        // the ordinary preparation/read-set proof. No interface body is opened.
        let contributors = self.global_contributors_in(
            root.canonical_id.as_ref(),
            root.symbol_name.as_ref(),
            verter_session_query::facts::SymbolSpace::Type,
        );
        for entry in contributors.entries.iter() {
            crate::resolver_core::resolver_context::observe_fan_out(
                verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
                    canonical_id: Arc::clone(&entry.artifact_key.canonical),
                    hash: entry.artifact_key.content_hash,
                },
            );
            let identity = DeclIdentity {
                canonical_id: entry.artifact_key.canonical.clone().into(),
                owner: entry.owner,
                whole_hash: entry.artifact_key.content_hash,
                decl_name: root.symbol_name.clone(),
            };
            let declaration = self.prepared_type_decl(&identity)?;
            if declaration
                .unique_symbol_members
                .iter()
                .any(|name| name == member)
            {
                return Some(UniqueSymbolMemberCertification::NamedType(
                    identity.canonical_id,
                    identity.owner,
                    identity.decl_name,
                ));
            }
            if declaration.member_index.contains_key(&key) {
                return None;
            }
        }
        None
    }

    /// Whether a value declaration's OWN static surface declares a property
    /// with this name (any annotation) — the heritage shadow stop for the
    /// member nominal rail. Reads the constructor-shape fact a class
    /// declaration already carries; non-class values have no heritage bases
    /// to shadow, so an absent shape answers `false` and costs nothing.
    fn value_decl_declares_own_member(
        &self,
        prepared: &verter_session_query::type_solver::PreparedValueDecl,
        member: &str,
    ) -> bool {
        prepared.object_shape.as_ref().is_some_and(|shape| {
            shape.members.iter().any(|fact| {
                matches!(
                    fact,
                    verter_type_expr::facts::ObjectMemberFact::Property(property)
                        if property.string_name() == Some(member)
                )
            })
        })
    }

    /// Lower an authored `typeof value` key through the ordinary type-domain
    /// path and recover the declaration's nominal identity from its result.
    ///
    /// This deliberately shares the full single-root-first lookup, namespace
    /// fallback, and trailing [`SemanticQueryKey::ProjectPath`] projection of
    /// the main [`TypeExpr::TypeOf`] arm. Only a bare name or one qualified
    /// member may certify a nominal key; deeper projections stay computed even
    /// when an intermediate prefix is nominal. Keeping a second path parser
    /// here can truncate qualified references or certify a shadowed namespace
    /// prefix.
    #[allow(clippy::too_many_arguments)]
    fn lower_typeof_for_authored_key(
        &self,
        computed: &TypeExpr,
        infer_binders: &crate::semantic_query::InferBinderFactory,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        name_resolution: &FxHashMap<verter_type_expr::OwnedStr, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        shadowing: &ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        reduction_context: ProjectionReductionContext,
    ) -> (
        SemanticNodeId,
        Option<verter_type_expr::facts::ValueDeclIdentityPart>,
    ) {
        let node = self.lower_type_expr_with_infer_factory(
            infer_binders,
            computed,
            env,
            scope,
            name_resolution,
            scope_payload,
            shadowing,
            substitutions,
            reduction_context,
        );
        let identity = match computed {
            // The MARKER on the produced node is the authority for "this
            // reference denotes a `unique symbol` declaration"; the authored
            // path length is not. An arity test here would publish a marked
            // carrier under `Computed` for the reference shapes it excludes,
            // giving one declaration two key representations that never merge.
            // Instantiation arguments still disqualify the key: `typeof
            // C.make<string>` names an instantiation, not a declaration.
            TypeExpr::TypeOf(value_ref) if value_ref.type_args.is_empty() => {
                self.unique_symbol_identity_for_typeof_node(node)
            }
            _ => None,
        };
        (node, identity)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_authored_property_key(
        &self,
        key: &verter_type_expr::TypeAuthoredPropertyKey,
        infer_binders: &crate::semantic_query::InferBinderFactory,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        name_resolution: &FxHashMap<verter_type_expr::OwnedStr, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        shadowing: &ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        reduction_context: ProjectionReductionContext,
    ) -> crate::semantic_query::AuthoredPropertyKey {
        match key {
            verter_type_expr::AuthoredPropertyKey::String(value) => {
                verter_type_expr::AuthoredPropertyKey::String(value.clone())
            }
            verter_type_expr::AuthoredPropertyKey::Number(value) => {
                verter_type_expr::AuthoredPropertyKey::Number(*value)
            }
            verter_type_expr::AuthoredPropertyKey::UniqueSymbol(identity) => {
                verter_type_expr::AuthoredPropertyKey::UniqueSymbol(identity.clone())
            }
            verter_type_expr::AuthoredPropertyKey::Computed(computed @ TypeExpr::TypeOf(_)) => {
                let (node, identity) = self.lower_typeof_for_authored_key(
                    computed,
                    infer_binders,
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    reduction_context,
                );
                identity
                    .map(verter_type_expr::AuthoredPropertyKey::UniqueSymbol)
                    .unwrap_or(verter_type_expr::AuthoredPropertyKey::Computed(node))
            }
            verter_type_expr::AuthoredPropertyKey::Computed(computed) => {
                verter_type_expr::AuthoredPropertyKey::Computed(
                    self.lower_type_expr_with_infer_factory(
                        infer_binders,
                        computed,
                        env,
                        scope,
                        name_resolution,
                        scope_payload,
                        shadowing,
                        substitutions,
                        reduction_context,
                    ),
                )
            }
        }
    }

    /// The literal type a dotted type-position reference `Enum.Member`
    /// names — the enum member's own nominal literal type — GATED strictly
    /// on the typed
    /// [`ValueDeclKind::Enum`](verter_session_query::declarations::ValueDeclKind::Enum)
    /// fact. Returns `None` for any prefix that is not a proven enum value
    /// declaration (so a non-enum `Ns.Member` reference is never
    /// mis-projected) or an unknown member name. A one-segment prefix
    /// resolves through the prepared decl's pre-resolved map first, then
    /// the shared bare-name resolver, then the global value declarations,
    /// so a locally-declared, an imported OR a global enum binding
    /// resolves; a qualified prefix (`Ns.Enum.Member`)
    /// resolves as a value path does, so a namespace's enum resolves in the
    /// namespace's own file.
    pub(super) fn resolve_enum_member_type(
        &self,
        scope: &NodeScopeId,
        name_resolution: &FxHashMap<verter_type_expr::OwnedStr, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        dotted_name: &str,
    ) -> Option<SemanticNodeId> {
        let NodeScopeId::File {
            canonical_id,
            owner,
            ..
        } = scope
        else {
            return None;
        };
        let scope_canonical = canonical_id.as_ref();
        let scope_owner = *owner;
        let (prefix, member) = dotted_name.rsplit_once('.')?;
        if let Some((specifier, qualified)) =
            self.ambient_namespace_target(scope, prefix, super::build::GlobalSpace::Value)
        {
            let augmentation =
                verter_session_query::declarations::AugmentationScopeKind::Module(specifier);
            let enumeration = self.enum_declaration_in_augmentation(
                scope_canonical,
                scope_owner,
                &qualified,
                Some(&augmentation),
            )?;
            return self.enum_member_type(&enumeration, member);
        }
        let (head, rest) = prefix.split_once('.').unwrap_or((prefix, ""));
        let indexed = self
            .ctx
            .ensure_indexed_ready_serve(scope_canonical)?
            .indexed;
        if let Some(verter_session_query::inputs::shallow::LexicalValueBinding::Import(target)) =
            indexed
                .shallow_state
                .visible_value_binding(scope_owner, head)
        {
            if self
                .ctx
                .resolve_type_dependency_canonical(scope_canonical, &target.source_specifier)
                .is_none()
            {
                let path: Vec<verter_type_expr::OwnedStr> = if rest.is_empty() {
                    Vec::new()
                } else {
                    rest.split('.')
                        .map(verter_type_expr::OwnedStr::from)
                        .collect()
                };
                let (root, consumed) =
                    self.ambient_import_member_root(scope_canonical, target, &path)?;
                if consumed != path.len() {
                    return None;
                }
                let indexed = self
                    .ctx
                    .ensure_indexed_ready_serve(&root.scope.canonical_id)?
                    .indexed;
                let augmentation =
                    Self::ambient_value_scope(&root.scope, &indexed.shallow_state.headers);
                let enumeration = self.enum_declaration_in_augmentation(
                    &root.scope.canonical_id,
                    root.scope.owner,
                    &root.name,
                    augmentation,
                )?;
                return self.enum_member_type(&enumeration, member);
            }
        }
        let identity = match prefix.split_once('.') {
            None => match name_resolution.get(prefix) {
                Some(direct) => direct.clone(),
                // A name the scope does not bind is the GLOBAL enum of
                // that name, a script's.
                None => resolve_bare_name_in_scope(
                    self.ctx,
                    self,
                    scope_canonical,
                    scope_owner,
                    scope_payload,
                    prefix,
                )
                .or_else(|| {
                    self.global_value_declaration(scope_canonical, prefix)
                        .map(|(identity, _)| identity)
                })?,
            },
            Some((root, rest)) => {
                let value_root = crate::semantic_query::ValueRootKey {
                    scope: crate::semantic_query::ScopeId {
                        canonical_id: verter_type_expr::OwnedStr::from(scope_canonical),
                        owner: scope_owner,
                        local_scope: None,
                        binder_scope_id: crate::semantic_query::BinderScopeId::file_scope(
                            scope_owner,
                        ),
                    },
                    name: verter_type_expr::OwnedStr::from(root),
                };
                let path: Vec<verter_type_expr::OwnedStr> = rest
                    .split('.')
                    .map(verter_type_expr::OwnedStr::from)
                    .collect();
                let (identity, remaining) = self.value_path_declaration(
                    &value_root,
                    &path,
                    &mut rustc_hash::FxHashSet::default(),
                )?;
                if !remaining.is_empty() {
                    return None;
                }
                identity
            }
        };
        // The enum resolves through the SAME export-target chase
        // `typeof Enum` uses, so a re-exported enum names its declaring
        // file's members.
        let enumeration = self.enum_declaration(
            identity.canonical_id.as_ref(),
            identity.owner,
            identity.symbol_name.as_ref(),
        )?;
        self.enum_member_type(&enumeration, member)
    }

    /// The ONE script-setup generic `TypeParam` node construction. BOTH
    /// query-time content readers route here — the eager shallow-lower
    /// `TypeExpr::Ref` arm below and the locator-view worklist projection
    /// (`locator_view_worklist/finish.rs`) — so the binder's identity and
    /// bound lowering can never diverge.
    ///
    /// The identity tuple is EXACTLY the historical one: the lowering
    /// scope's canonical / owner / whole hash, the `"<script-setup>"`
    /// sentinel, the stored clause ordinal, and the display name.
    ///
    /// Bound CONTENT is never stored: the full
    /// `<script setup generic="…">` clause is re-borrowed lease-only from
    /// the pinned `IndexedReady` through the ONE artifact-local transient
    /// producer
    /// ([`crate::host_resolve::indexed_script_setup_type_params`]), and the
    /// binding's stored `(ordinal, name)` selects + validates the transient
    /// parameter. Canonical / owner / whole-hash coherence with the
    /// lowering scope is required: a missing serve (a `Global` scope has no
    /// file to re-borrow from), a superseded artifact (hash drift), or a
    /// stale clause (ordinal / name mismatch) is a TYPED MISS with cache
    /// suppression — NEVER a bound-free fabricated binder.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::project_semantic_dispatch) fn lower_script_setup_type_param_binding(
        &self,
        binding: &verter_session_query::inputs::prepared::TypeParamBinding,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        name_resolution: &FxHashMap<verter_type_expr::OwnedStr, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        shadowing: &ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        reduction_context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let decl = match scope {
            NodeScopeId::Global => DeclIdentity {
                canonical_id: verter_type_expr::OwnedStr::from(""),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                whole_hash: HashValue::default(),
                decl_name: verter_type_expr::OwnedStr::from("<script-setup>"),
            },
            NodeScopeId::File {
                canonical_id,
                owner,
                whole_hash,
                ..
            } => DeclIdentity {
                canonical_id: canonical_id.clone(),
                owner: *owner,
                whole_hash: *whole_hash,
                decl_name: verter_type_expr::OwnedStr::from("<script-setup>"),
            },
        };
        let transient_param = match scope {
            NodeScopeId::Global => None,
            NodeScopeId::File {
                canonical_id,
                whole_hash,
                ..
            } => self
                .ctx
                .ensure_indexed_ready_serve(canonical_id.as_ref())
                .filter(|serve| serve.indexed.whole_hash == *whole_hash)
                .and_then(|serve| {
                    self.ctx
                        .script_setup_type_params(&serve)
                        .into_iter()
                        .nth(binding.ordinal as usize)
                        .filter(|param| param.name == binding.name.as_ref())
                }),
        };
        let Some(param) = transient_param else {
            crate::fact_tracing::note_non_cacheable_read_fan_out(
                verter_session_query::facts::reuse::NonCacheableReadReason::UnobservableSource,
            );
            return self.opaque(QueryError::Miss);
        };
        let bound_locator = |position| {
            let NodeScopeId::File {
                canonical_id,
                owner,
                ..
            } = scope
            else {
                unreachable!("script-setup bounds require their authored file scope");
            };
            verter_type_expr::locators::AuthoredBodyLocator::DeclBody(
                verter_type_expr::locators::TypeBodySlot {
                    anchor: verter_type_expr::locators::AuthoredAnchor {
                        canonical_id: verter_type_expr::OwnedStr::from(canonical_id),
                        owner: *owner,
                        symbol: verter_type_expr::OwnedStr::from("<script-setup>"),
                        space: verter_type_expr::locators::LocatorSymbolSpace::Type,
                    },
                    path: verter_type_expr::OwnedSlice::from(
                        vec![
                            verter_type_expr::locators::TypeBodyPathStep::TypeParamBound {
                                ordinal: u32::from(binding.ordinal),
                                position,
                            },
                        ]
                        .into_boxed_slice(),
                    ),
                },
            )
        };
        let constraint = param.constraint.as_ref().map(|constraint| {
            self.shallow_lower_type_expr_with_context_at_locator(
                constraint,
                &bound_locator(verter_type_expr::locators::TypeParamBoundPosition::Constraint),
                env,
                scope,
                name_resolution,
                scope_payload,
                shadowing,
                substitutions,
                reduction_context,
            )
        });
        let default = param.default.as_ref().map(|default| {
            self.shallow_lower_type_expr_with_context_at_locator(
                default,
                &bound_locator(verter_type_expr::locators::TypeParamBoundPosition::Default),
                env,
                scope,
                name_resolution,
                scope_payload,
                shadowing,
                substitutions,
                reduction_context,
            )
        });
        graph.intern_node_with_scope(
            SemanticNodeData::TypeParam {
                decl,
                param_index: binding.ordinal,
                constraint,
                default,
                display_name: OwnedStr::from(&binding.name),
            },
            scope.clone(),
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    pub fn lower_script_setup_type_params_for_tests(&self, canonical: &str) -> Vec<SemanticNodeId> {
        let indexed = self
            .ctx
            .ensure_indexed_ready_serve(canonical)
            .expect("script-setup test file must be indexed")
            .indexed;
        let bundle = self
            .ctx
            .prepared_decl_bundle(canonical)
            .expect("script-setup test file must have a prepared bundle");
        let (owner, owner_scope) = bundle
            .owner_scopes
            .iter()
            .find(|(_, scope)| !scope.script_setup_type_bindings.is_empty())
            .expect("script-setup owner scope");
        let owner = *owner;
        let scope = NodeScopeId::File {
            canonical_id: verter_type_expr::OwnedStr::from(canonical),
            owner,
            whole_hash: indexed.whole_hash,
            local_scope: None,
        };
        let scope_payload =
            crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(
                &bundle, owner,
            );
        let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(
            Some(&scope_payload),
        );
        let env = FxHashMap::default();
        let name_resolution = FxHashMap::default();
        let mut substitutions = Vec::new();
        let mut bindings: Vec<_> = owner_scope.script_setup_type_bindings.values().collect();
        bindings.sort_by_key(|binding| binding.ordinal);
        bindings
            .into_iter()
            .map(|binding| {
                self.lower_script_setup_type_param_binding(
                    binding,
                    &env,
                    &scope,
                    &name_resolution,
                    Some(&scope_payload),
                    &shadowing,
                    &mut substitutions,
                    ProjectionReductionContext::structural_transit(),
                )
            })
            .collect()
    }

    /// Shallow-lower a [`TypeExpr`] under `env` (type-parameter bindings)
    /// into a [`SemanticNodeId`]. "Shallow" means one structural level:
    /// object members, union/intersection arms, and function / conditional
    /// sub-expressions are interned as references rather than recursively
    /// expanded. Deeper lowering is the caller's responsibility via
    /// [`SemanticQueryKey::ProjectPath`] sub-queries.
    ///
    /// Accepts the full [`ProjectionReductionContext`] so callers thread
    /// their demand through nested lowering: a `StructuralTransit`
    /// instantiation lowers its body with the same demand and nested
    /// operator dispatches carrier-stop.
    ///
    /// `name_resolution` is the prepared decl's bare-name → canonical
    /// identity map; used by the walker to resolve `TypeExpr::Ref`
    /// hops through `ResolveDecl` or nested `Instantiate` sub-shells
    /// via `SemanticQueryApi::execute`.
    ///
    /// `scope_payload` carries the owning file's declaration-scope
    /// payload (script-setup type bindings, scope-local type/value
    /// names, import bindings). It is consulted when the bare `Ref`
    /// name is NOT in `name_resolution` — the walker falls through to
    /// [`resolve_bare_name_in_scope`] which looks at host-owned
    /// `shallow_file_state` + prepared-decl bundle + export-target
    /// resolvers ( — dispatch carries full
    /// name-resolution context without routing through
    /// `SessionSolverHost`).
    ///
    /// `shadowing` carries the scope-shadowing decision once per
    /// resolver context. The dispatch
    /// fast-path consults `shadowing.is_shadowing_lib(name)` before
    /// routing a bare `Ref` through the ambient-lib `__builtin__`
    /// path — when `true`, the userland declaration wins via the
    /// standard `ResolveDecl` route. The struct (rather than a bare
    /// `bool`) keeps the threading axis single-source-of-truth so the
    /// `ResolverContext` absorbs the field without inventing a
    /// parallel axis to undo. Constructible via
    /// [`ScopeShadowing::from_scope_payload`] (dispatch path) or
    /// [`ScopeShadowing::from_host_scope`] (materialise path).
    ///
    /// `substitutions` accumulates `(param_name, arg_id)` facts for
    /// `SubstituteTypeParam` origin-edge emission at the shell level.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn shallow_lower_type_expr_with_context(
        &self,
        expr: &TypeExpr,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        name_resolution: &FxHashMap<verter_type_expr::OwnedStr, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        shadowing: &ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        reduction_context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        let infer_binders = crate::semantic_query::InferBinderFactory::new(scope, expr);
        self.lower_type_expr_with_infer_factory(
            &infer_binders,
            expr,
            env,
            scope,
            name_resolution,
            scope_payload,
            shadowing,
            substitutions,
            reduction_context,
        )
    }

    /// Test-support entry to [`Self::shallow_lower_type_expr_with_context`]
    /// for the host's lowering suites; the reducing lowerer stays
    /// crate-private.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn shallow_lower_type_expr_with_context_for_tests(
        &self,
        expr: &TypeExpr,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        name_resolution: &FxHashMap<verter_type_expr::OwnedStr, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        shadowing: &ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        reduction_context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        self.shallow_lower_type_expr_with_context(
            expr,
            env,
            scope,
            name_resolution,
            scope_payload,
            shadowing,
            substitutions,
            reduction_context,
        )
    }

    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    pub fn step_type_lowering_for_tests(
        &self,
        expr: &TypeExpr,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        name_resolution: &FxHashMap<verter_type_expr::OwnedStr, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        shadowing: &ScopeShadowing,
        reduction_context: ProjectionReductionContext,
    ) -> TypeLoweringTrace {
        let lowering = TypeExprLowering {
            env,
            scope,
            name_resolution,
            scope_payload,
            shadowing,
        };
        let mut read = self.begin_type_lowering(expr, scope, reduction_context);
        let mut requested = Vec::new();
        let mut progress = self.advance_type_lowering(&lowering, &mut read, None);
        let first_advance_done = matches!(progress, LowerProgress::Done(_));
        loop {
            match progress {
                LowerProgress::Done(node) => {
                    return TypeLoweringTrace {
                        node,
                        requested,
                        substitutions: read.substitutions,
                        first_advance_done,
                    }
                }
                LowerProgress::Read(request) => {
                    requested.push((request.key.clone(), request.shape));
                    let delivered = self.answer_lower_read(request);
                    progress = self.advance_type_lowering(&lowering, &mut read, Some(delivered));
                }
            }
        }
    }

    construction_ordinal_seam! {
        /// Locator-anchored eager lowering for an authored payload that has
        /// already been re-borrowed and validated by its producer.
        #[allow(clippy::too_many_arguments)]
        fn shallow_lower_type_expr_with_context_at_locator(
            &self,
            expr: &TypeExpr,
            locator: &verter_type_expr::locators::AuthoredBodyLocator,
            env: &FxHashMap<String, SemanticNodeId>,
            scope: &NodeScopeId,
            name_resolution: &FxHashMap<verter_type_expr::OwnedStr, ResolvedRootIdentity>,
            scope_payload: Option<&DeclarationScopePayload>,
            shadowing: &ScopeShadowing,
            substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
            reduction_context: ProjectionReductionContext,
        ) -> SemanticNodeId {
            let infer_binders =
                crate::semantic_query::InferBinderFactory::for_authored_locator(scope, expr, locator);
            self.lower_type_expr_with_infer_factory(
                &infer_binders,
                expr,
                env,
                scope,
                name_resolution,
                scope_payload,
                shadowing,
                substitutions,
                reduction_context,
            )
        }
    }

    /// Lower `expr` — the ONE eager lowering entry every arm recurses
    /// through.
    ///
    /// A type's syntax nests without bound (`Box<Box<…<1>>>`,
    /// `T['a']['b']…`, `{ a: { a: … } }`), so the positions that lower a
    /// child under this lowering's own scope, binder environment and infer
    /// factory — a named reference's arguments, union and intersection
    /// arms, an array's element, tuple elements, a template's holes, a
    /// parenthesised type, a `keyof` operand, an indexed access's object
    /// and index, `typeof` and import-type arguments, function/constructor
    /// children, mapped children, conditional operands and infer constraints,
    /// and an object type's property values and index signatures — are
    /// lowered from an explicit stack of [`LowerFrame`]s: each node records what
    /// remains of its own lowering and resumes when its child's value is
    /// delivered. Children are lowered in the order, and under the
    /// contexts, the node's own lowering demands them, so every intern,
    /// query and substitution happens in the same sequence as a recursive
    /// descent would make it. Conditional decisions and branch callbacks stay
    /// in place; other leaf forms use [`Self::lower_type_expr_leaf`].
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_type_expr_with_infer_factory(
        &self,
        infer_binders: &crate::semantic_query::InferBinderFactory,
        expr: &TypeExpr,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        name_resolution: &FxHashMap<verter_type_expr::OwnedStr, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        shadowing: &ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        reduction_context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        let lowering = TypeExprLowering {
            env,
            scope,
            name_resolution,
            scope_payload,
            shadowing,
        };
        let mut cursor = TypeLowerCursor::new(expr, reduction_context);
        let mut delivered = None;
        loop {
            match self.advance_type_lowering_cursor(
                &lowering,
                infer_binders,
                &mut cursor,
                substitutions,
                delivered.take(),
            ) {
                LowerProgress::Done(node) => return node,
                LowerProgress::Read(request) => delivered = Some(self.answer_lower_read(request)),
            }
        }
    }

    pub(super) fn begin_type_lowering<'e>(
        &self,
        expr: &'e TypeExpr,
        scope: &NodeScopeId,
        context: ProjectionReductionContext,
    ) -> TypeLowerRead<'e> {
        TypeLowerRead {
            cursor: TypeLowerCursor::new(expr, context),
            infer_binders: crate::semantic_query::InferBinderFactory::new(scope, expr),
            substitutions: Vec::new(),
        }
    }

    /// Lend the context for one advance; no dispatcher borrow survives it.
    pub(super) fn advance_type_lowering(
        &self,
        lowering: &TypeExprLowering<'_>,
        read: &mut TypeLowerRead<'_>,
        delivered: Option<LowerDelivery>,
    ) -> LowerProgress {
        self.advance_type_lowering_cursor(
            lowering,
            &read.infer_binders,
            &mut read.cursor,
            &mut read.substitutions,
            delivered,
        )
    }

    /// The single in-place answerer for both lowering entry points.
    pub(super) fn answer_lower_read(&self, request: LowerRead) -> LowerDelivery {
        match request.shape {
            LowerReadShape::Node => LowerDelivery::Node(self.execute_type_node(request.key)),
            LowerReadShape::Whole => LowerDelivery::Whole(self.execute_read(request.key)),
        }
    }

    pub(super) fn drive_type_lowering(
        &self,
        lowering: &TypeExprLowering<'_>,
        read: &mut TypeLowerRead<'_>,
    ) -> SemanticNodeId {
        let mut delivered = None;
        loop {
            match self.advance_type_lowering(lowering, read, delivered.take()) {
                LowerProgress::Done(node) => return node,
                LowerProgress::Read(request) => delivered = Some(self.answer_lower_read(request)),
            }
        }
    }

    fn advance_type_lowering_cursor(
        &self,
        lowering: &TypeExprLowering<'_>,
        infer_binders: &crate::semantic_query::InferBinderFactory,
        cursor: &mut TypeLowerCursor<'_>,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        delivered: Option<LowerDelivery>,
    ) -> LowerProgress {
        let mut step = if let Some(delivery) = delivered {
            let Some(LowerFrame::Read(resume)) = cursor.frames.pop() else {
                panic!("lowering received an answer without a pending read");
            };
            let scoped = TypeExprLowering {
                env: cursor.environment.as_deref().unwrap_or(lowering.env),
                ..*lowering
            };
            self.resume_lower_read(&scoped, resume, delivery, &mut cursor.frames)
        } else {
            let (expr, context) = cursor
                .next
                .take()
                .expect("lowering needs its pending answer");
            let scoped = TypeExprLowering {
                env: cursor.environment.as_deref().unwrap_or(lowering.env),
                ..*lowering
            };
            self.lower_type_expr_step(
                &scoped,
                infer_binders,
                expr,
                context,
                substitutions,
                &mut cursor.frames,
            )
        };
        loop {
            match step {
                LowerStep::Read(request) => return LowerProgress::Read(request),
                LowerStep::Scoped(expr, context, env) => {
                    let previous = cursor.environment.replace(env);
                    cursor.frames.push(LowerFrame::Environment(previous));
                    step = LowerStep::Descend(expr, context);
                }
                LowerStep::Descend(expr, context) => {
                    let scoped = TypeExprLowering {
                        env: cursor.environment.as_deref().unwrap_or(lowering.env),
                        ..*lowering
                    };
                    step = self.lower_type_expr_step(
                        &scoped,
                        infer_binders,
                        expr,
                        context,
                        substitutions,
                        &mut cursor.frames,
                    );
                }
                LowerStep::Value(value) => {
                    let Some(frame) = cursor.frames.pop() else {
                        return LowerProgress::Done(value);
                    };
                    if let LowerFrame::Environment(previous) = frame {
                        cursor.environment = previous;
                        step = LowerStep::Value(value);
                        continue;
                    }
                    let scoped = TypeExprLowering {
                        env: cursor.environment.as_deref().unwrap_or(lowering.env),
                        ..*lowering
                    };
                    step = self.resume_type_expr_frame(
                        &scoped,
                        infer_binders,
                        frame,
                        value,
                        substitutions,
                        &mut cursor.frames,
                    );
                }
            }
        }
    }

    fn resume_lower_read<'e>(
        &self,
        lowering: &TypeExprLowering<'_>,
        resume: LowerReadResume<'e>,
        delivery: LowerDelivery,
        frames: &mut LowerFrames<'e>,
    ) -> LowerStep<'e> {
        match (resume, delivery) {
            (LowerReadResume::ImportTail, LowerDelivery::Whole(read)) => {
                LowerStep::Value(self.carrier_resolution_of(read).node)
            }
            (
                LowerReadResume::ImportAnchor {
                    anchor,
                    arguments,
                    context,
                },
                LowerDelivery::Whole(read),
            ) => {
                let answer = self.carrier_resolution_of(read);
                match self.resume_anchor(*anchor, answer) {
                    super::carrier::CarrierResolutionPlan::Ready(value) => {
                        LowerStep::Value(value.node)
                    }
                    super::carrier::CarrierResolutionPlan::NeedsArgs(continuation) => {
                        self.lower_import_arguments(continuation, arguments, context, frames)
                    }
                }
            }
            (
                LowerReadResume::Anchor {
                    anchor,
                    arguments,
                    context,
                },
                LowerDelivery::Whole(read),
            ) => {
                let answer = self.carrier_resolution_of(read);
                match self.resume_anchor(*anchor, answer) {
                    super::carrier::CarrierResolutionPlan::Ready(value) => {
                        LowerStep::Value(value.node)
                    }
                    super::carrier::CarrierResolutionPlan::NeedsArgs(continuation) => {
                        self.lower_ref_arguments(continuation, arguments, context, frames)
                    }
                }
            }
            (LowerReadResume::CarrierFinish(finish), LowerDelivery::Whole(read)) => {
                let answer = self.carrier_resolution_of(read);
                LowerStep::Value(self.resume_carrier_finish(finish, answer).node)
            }
            (LowerReadResume::MappedKeySpace(mut state), LowerDelivery::Node(answer)) => {
                state.key_space = Some(match answer {
                    QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
                    _ => self.opaque(QueryError::Miss),
                });
                self.mapped_value_step(lowering, state, frames)
            }
            (LowerReadResume::KeyOf, LowerDelivery::Node(answer)) => {
                LowerStep::Value(match answer {
                    QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
                    _ => self.opaque(QueryError::Miss),
                })
            }
            (
                LowerReadResume::Indexed {
                    object,
                    index,
                    origin,
                    context,
                },
                LowerDelivery::Node(answer),
            ) => LowerStep::Value(match answer {
                QueryResult::Value(SemanticQueryOutput { value, .. }) => self
                    .absent_read_answer(origin, value, object, &index, context)
                    .unwrap_or(value),
                _ => self.opaque(QueryError::Miss),
            }),
            (LowerReadResume::Typeof { arguments, context }, LowerDelivery::Node(answer)) => {
                let QueryResult::Value(SemanticQueryOutput { value: base, .. }) = answer else {
                    return LowerStep::Value(self.opaque(QueryError::Miss));
                };
                match arguments.first() {
                    None => LowerStep::Value(base),
                    Some(first) => {
                        frames.push(LowerFrame::TypeofArguments {
                            base,
                            arguments,
                            context,
                            lowered: Vec::with_capacity(arguments.len()),
                        });
                        LowerStep::Descend(first, context)
                    }
                }
            }
            _ => panic!("lowering read delivered the wrong answer shape"),
        }
    }

    fn lower_carrier_finish<'e>(
        &self,
        continuation: super::carrier::CarrierArgsContinuation,
        args: Arc<[SemanticNodeId]>,
        frames: &mut LowerFrames<'e>,
    ) -> LowerStep<'e> {
        match self.plan_carrier_resolution_finish(continuation, args) {
            super::carrier::CarrierFinishPlan::Ready(value) => LowerStep::Value(value.node),
            super::carrier::CarrierFinishPlan::Read { key, finish } => {
                self.record_carrier_resolution_read_intent(&key);
                frames.push(LowerFrame::Read(LowerReadResume::CarrierFinish(finish)));
                LowerStep::Read(LowerRead {
                    key,
                    shape: LowerReadShape::Whole,
                })
            }
        }
    }

    /// Begin lowering one node: a structural position pushes its frame and
    /// descends into its first child (see
    /// [`Self::lower_type_expr_with_infer_factory`]); every other form is
    /// lowered in place.
    fn lower_type_expr_step<'e>(
        &self,
        lowering: &TypeExprLowering<'_>,
        infer_binders: &crate::semantic_query::InferBinderFactory,
        expr: &'e TypeExpr,
        reduction_context: ProjectionReductionContext,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        frames: &mut LowerFrames<'e>,
    ) -> LowerStep<'e> {
        // Captured execution can participate in hang investigation:
        // advance its heartbeat and answer a stall signal with a
        // self-backtrace. Uncaptured execution reads no watchdog state.
        // See `loop5_instrumentation.rs` watchdog module.
        #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
        if <C::Observe as verter_audit::observe::ObserveExec>::CAPTURED {
            crate::loop5_instrumentation::watchdog_beat();
            crate::loop5_instrumentation::watchdog_check_and_dump("shallow_lower_type_expr");
        }
        let graph = self.graph();
        graph.record_decl_subexpression_lowering();
        match expr {
            TypeExpr::Function(func) | TypeExpr::ConstructorType(func) => {
                self.begin_function_lowering(lowering, expr, func, reduction_context, frames)
            }
            TypeExpr::Mapped { .. } => {
                self.begin_mapped_lowering(lowering, expr, reduction_context, frames)
            }
            TypeExpr::Conditional { check, .. } => {
                if self.cancellation.is_cancelled() {
                    return LowerStep::Value(self.opaque(QueryError::Miss));
                }
                let mut head = check.as_ref();
                while let TypeExpr::Parenthesized(inner) = head {
                    head = inner;
                }
                let context = if matches!(head, TypeExpr::TypeOf { .. }) {
                    reduction_context
                } else {
                    Self::conditional_operand_context(reduction_context)
                };
                frames.push(LowerFrame::Conditional(
                    ConditionalLowering::new(expr, reduction_context),
                    ConditionalAwait::Check,
                ));
                LowerStep::Descend(check, context)
            }
            TypeExpr::ImportType {
                specifier,
                qualifier,
                typeof_query,
                type_arguments,
            } => {
                let NodeScopeId::File { canonical_id, .. } = lowering.scope else {
                    return LowerStep::Value(self.opaque(QueryError::Miss));
                };
                let ctx = super::carrier::CarrierResolverContext::new(
                    lowering.env,
                    lowering.scope,
                    lowering.name_resolution,
                    lowering.scope_payload,
                    lowering.shadowing,
                    reduction_context,
                );
                let plan = self.plan_import_type_head_steps(
                    &ctx,
                    canonical_id.as_ref(),
                    specifier,
                    &verter_type_expr::OwnedSlice::<OwnedStr>::from(qualifier.as_ref()),
                    *typeof_query,
                    type_arguments.len(),
                );
                if let Some(tail) = plan.tail {
                    frames.push(LowerFrame::ImportTail(tail));
                }
                match plan.head {
                    super::carrier::HeadPlan::Ready(value) => LowerStep::Value(value.node),
                    super::carrier::HeadPlan::NeedsArgs(continuation) => self
                        .lower_import_arguments(
                            continuation,
                            type_arguments,
                            reduction_context,
                            frames,
                        ),
                    super::carrier::HeadPlan::ReadsAnchor(anchor) => {
                        let key = anchor.key().clone();
                        self.record_carrier_resolution_read_intent(&key);
                        // Import arguments retain the caller's context, unlike bare-reference operands.
                        frames.push(LowerFrame::Read(LowerReadResume::ImportAnchor {
                            anchor: Box::new(anchor),
                            arguments: type_arguments,
                            context: reduction_context,
                        }));
                        LowerStep::Read(LowerRead {
                            key,
                            shape: LowerReadShape::Whole,
                        })
                    }
                }
            }
            // Named type reference (`type Foo<T> = { y: Other<T> }` ->
            // `Other<T>` at `y`'s position) that neither substitutes a bound
            // parameter nor names a script-setup generic parameter (those
            // are leaves). The head resolves FIRST through the shared
            // `plan_bare_ref_head` resolver -- the ONE bare-name resolver,
            // equally reached from carrier-subject normalization -- which
            // performs builtin-shadowing-aware utility / Promise carriers,
            // the bare-name + augmentation + enum resolution, the recursive-ref
            // back-edge, and the route through `DeclRef` / `InstantiationRef`
            // (carrier modes) or `ResolveDecl` / `Instantiate` (eager modes).
            // The type-args lower LAZILY: an unresolvable head never lowers
            // dead args; they are lowered only when the plan's continuation
            // consumes them (substituting INTO the resolved decl body, never
            // the macro-T own body). Self-referential types are bounded by
            // the memo's same-path recursion sentinel.
            TypeExpr::Ref {
                name,
                type_arguments,
            } if lowering.resolves_through_head(name, type_arguments) => {
                let ctx = crate::project_semantic_dispatch::carrier::CarrierResolverContext::new(
                    lowering.env,
                    lowering.scope,
                    lowering.name_resolution,
                    lowering.scope_payload,
                    lowering.shadowing,
                    reduction_context,
                );
                match self.plan_bare_ref_head_steps(&ctx, name, type_arguments.len()) {
                    super::carrier::HeadPlan::Ready(value) => LowerStep::Value(value.node),
                    super::carrier::HeadPlan::NeedsArgs(continuation) => self.lower_ref_arguments(
                        continuation,
                        type_arguments,
                        reduction_context,
                        frames,
                    ),
                    super::carrier::HeadPlan::ReadsAnchor(anchor) => {
                        let key = anchor.key().clone();
                        self.record_carrier_resolution_read_intent(&key);
                        frames.push(LowerFrame::Read(LowerReadResume::Anchor {
                            anchor: Box::new(anchor),
                            arguments: type_arguments,
                            context: reduction_context,
                        }));
                        LowerStep::Read(LowerRead {
                            key,
                            shape: LowerReadShape::Whole,
                        })
                    }
                }
            }
            TypeExpr::TypeOf(value_ref) => {
                let NodeScopeId::File {
                    canonical_id,
                    owner,
                    ..
                } = lowering.scope
                else {
                    return LowerStep::Value(self.opaque(QueryError::Miss));
                };
                let Some(key) = self.typeof_reference_key_in_scope(
                    canonical_id,
                    *owner,
                    Some(lowering.name_resolution),
                    lowering.scope_payload,
                    value_ref,
                    reduction_context,
                ) else {
                    return LowerStep::Value(self.opaque(QueryError::Miss));
                };
                frames.push(LowerFrame::Read(LowerReadResume::Typeof {
                    arguments: &value_ref.type_args,
                    context: reduction_context,
                }));
                LowerStep::Read(LowerRead {
                    key,
                    shape: LowerReadShape::Node,
                })
            }
            TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => match arms.first() {
                None => LowerStep::Value(
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never)),
                ),
                Some(first) => {
                    frames.push(LowerFrame::Composite {
                        arms,
                        union: matches!(expr, TypeExpr::Union(_)),
                        context: reduction_context,
                        lowered: Vec::with_capacity(arms.len()),
                    });
                    LowerStep::Descend(first, reduction_context)
                }
            },
            // Arrays publish through the dedicated
            // `SemanticNodeData::Array { element, readonly }` variant per
            // B4 + §7.14: array indexed-access is hot and must not
            // pay generic `Array<T>` declaration-instantiation cost on
            // every access.
            TypeExpr::Array { element, readonly } => {
                frames.push(LowerFrame::Array {
                    readonly: *readonly,
                });
                LowerStep::Descend(element, reduction_context)
            }
            // Tuples publish via `SemanticNodeData::Tuple` preserving
            // label / optional / rest metadata for every element (plan
            // §3 B4 + §7.14). Element bodies are lazily interned at
            // shell level — deeper expansion happens through
            // `ProjectPath` sub-queries when a caller reaches into a
            // specific slot.
            TypeExpr::Tuple { elements, readonly } => match elements.first() {
                None => LowerStep::Value(self.finish_tuple_lowering(
                    Vec::new(),
                    *readonly,
                    lowering.scope,
                )),
                Some(first) => {
                    frames.push(LowerFrame::Tuple {
                        elements,
                        readonly: *readonly,
                        context: reduction_context,
                        lowered: Vec::with_capacity(elements.len()),
                    });
                    LowerStep::Descend(&first.ty, reduction_context)
                }
            },
            // Template-literal shells publish verbatim — the relation
            // engine's infer-pattern support for template matching is a
            // follow-up per, but the shell carrier itself is
            // not deferred.
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => match expressions.first() {
                None => LowerStep::Value(self.finish_template_lowering(
                    quasis,
                    Vec::new(),
                    lowering.scope,
                )),
                Some(first) => {
                    frames.push(LowerFrame::Template {
                        quasis,
                        expressions,
                        context: reduction_context,
                        lowered: Vec::with_capacity(expressions.len()),
                    });
                    LowerStep::Descend(first, reduction_context)
                }
            },
            // Parenthesised types are structurally transparent — `(A | B)`
            // is equivalent to `A | B`. Unwrap and descend (plan B4
            // follow-up).
            TypeExpr::Parenthesized(inner) => LowerStep::Descend(inner, reduction_context),
            // KeyOf at shell level routes through the KeyOf dispatch.
            TypeExpr::KeyOf(operand) => {
                frames.push(LowerFrame::KeyOf {
                    context: reduction_context,
                });
                LowerStep::Descend(
                    operand,
                    reduction_context.into_structural_transit_with_mode(ProjectionMode::Navigate),
                )
            }
            // Indexed access at shell level routes through the IndexedAccess
            // dispatch. The path walker materialises `T[K]` via
            // `ProjectPath` semantics.
            TypeExpr::IndexedAccess { object, index } => {
                // The object is an operand, including on a single-hop read.
                // Keep mapped values and declaration interiors deferred until
                // the index selects them; the terminal retains the caller's
                // context in finish_indexed_access_lowering.
                let mut object_head = object.as_ref();
                while let TypeExpr::Parenthesized(inner) = object_head {
                    object_head = inner;
                }
                let object_context = if matches!(object_head, TypeExpr::Mapped { .. }) {
                    // The selected index, rather than object navigation,
                    // owns substitution and evaluation of mapped values.
                    reduction_context.into_structural_transit_with_mode(ProjectionMode::Navigate)
                } else if matches!(
                    object_head,
                    TypeExpr::Ref { .. } | TypeExpr::IndexedAccess { .. }
                ) {
                    // Lowering still needs the generic carrier's navigation
                    // context. The raise reducer's default transit/Shallow
                    // operand context would hide callable argument bindings.
                    reduction_context.with_mode(ProjectionMode::Navigate)
                } else if matches!(object_head, TypeExpr::TypeOf { .. }) {
                    // A value-root lookup owns its module export domain,
                    // including stable absence; it is not a type-argument hop.
                    reduction_context
                } else {
                    super::raise::indexed_access_object_context(reduction_context)
                };
                frames.push(LowerFrame::IndexedObject {
                    index,
                    context: reduction_context,
                });
                LowerStep::Descend(object, object_context)
            }
            // A spread-bearing object literal folds through the shared
            // spread materializer (ordered left fold over direct members
            // and spread operands) as a leaf.
            TypeExpr::Object(object)
                if !object
                    .properties
                    .iter()
                    .any(|member| matches!(member, ObjectMember::Spread(_))) =>
            {
                self.advance_object_lowering(
                    lowering,
                    infer_binders,
                    object,
                    0,
                    Vec::with_capacity(object.properties.len()),
                    reduction_context,
                    substitutions,
                    frames,
                )
            }
            _ => LowerStep::Value(self.lower_type_expr_leaf(
                infer_binders,
                expr,
                lowering.env,
                lowering.scope,
                lowering.name_resolution,
                lowering.scope_payload,
                lowering.shadowing,
                substitutions,
                reduction_context,
            )),
        }
    }

    fn resume_conditional_lowering<'e>(
        &self,
        lowering: &TypeExprLowering<'_>,
        infer_binders: &crate::semantic_query::InferBinderFactory,
        pending: (ConditionalLowering<'e>, ConditionalAwait),
        value: SemanticNodeId,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        frames: &mut LowerFrames<'e>,
    ) -> LowerStep<'e> {
        let (mut state, awaiting) = pending;
        let TypeExpr::Conditional { extends, .. } = state.expr else {
            unreachable!()
        };
        match awaiting {
            ConditionalAwait::Check => {
                state.check = Some(value);
                let path=infer_binders.path_for_expr(state.expr).child(crate::semantic_query::infer_binder_names::InferSyntaxPathStep::ConditionalExtends);
                state.sites =
                    crate::semantic_query::infer_binder_names::collect_extends_infer_declarations(
                        extends, &path,
                    );
                if !state.sites.is_empty() {
                    let recovery = crate::semantic_query::checker_policy::checker_recovery(
                        self.graph(),
                        crate::semantic_query::checker_policy::unresolved_type_name(),
                    );
                    let mut outside = lowering.env.clone();
                    for site in &state.sites {
                        outside.insert(
                            super::carrier::out_of_scope_infer_env_key(site.name.as_ref()),
                            recovery,
                        );
                    }
                    state.out_of_scope = Some(Arc::new(outside));
                    state.extends_env = Some(Arc::new(lowering.env.clone()));
                    state.declarations = Vec::with_capacity(state.sites.len());
                }
            }
            ConditionalAwait::Constraint => {
                self.commit_conditional_infer(lowering, &mut state, Some(value))
            }
            ConditionalAwait::Extends => {
                return LowerStep::Value(self.finish_conditional_lowering(
                    lowering,
                    infer_binders,
                    state,
                    value,
                    substitutions,
                ))
            }
        }
        self.advance_conditional_extends(lowering, infer_binders, state, frames)
    }

    fn commit_conditional_infer(
        &self,
        lowering: &TypeExprLowering<'_>,
        state: &mut ConditionalLowering<'_>,
        constraint: Option<SemanticNodeId>,
    ) {
        let site = &state.sites[state.next];
        let binder = state
            .pending_binder
            .take()
            .expect("infer binder selected before its constraint");
        let declaration = self.graph().intern_node_with_scope(
            SemanticNodeData::Infer {
                name: OwnedStr::from(&site.name),
                binder: binder.clone(),
                constraint,
            },
            lowering.scope.clone(),
        );
        Arc::make_mut(state.extends_env.as_mut().expect("infer environment"))
            .insert(infer_declaration_env_key(site.name.as_ref()), declaration);
        state.declarations.push((Arc::clone(&site.name), binder));
        state.next += 1;
    }

    fn advance_conditional_extends<'e>(
        &self,
        lowering: &TypeExprLowering<'_>,
        infer_binders: &crate::semantic_query::InferBinderFactory,
        mut state: ConditionalLowering<'e>,
        frames: &mut LowerFrames<'e>,
    ) -> LowerStep<'e> {
        let context = Self::conditional_target_context(state.context);
        while let Some(site) = state.sites.get(state.next) {
            state.pending_binder = Some(infer_binders.binder_at(&site.path));
            if let Some(constraint) = site.constraint {
                let env = Arc::clone(
                    state
                        .out_of_scope
                        .as_ref()
                        .expect("infer constraint environment"),
                );
                frames.push(LowerFrame::Conditional(state, ConditionalAwait::Constraint));
                return LowerStep::Scoped(constraint, context, env);
            }
            self.commit_conditional_infer(lowering, &mut state, None);
        }
        let TypeExpr::Conditional { extends, .. } = state.expr else {
            unreachable!()
        };
        let env = state.extends_env.clone();
        frames.push(LowerFrame::Conditional(state, ConditionalAwait::Extends));
        match env {
            Some(env) => LowerStep::Scoped(extends, context, env),
            None => LowerStep::Descend(extends, context),
        }
    }

    fn finish_conditional_lowering(
        &self,
        lowering: &TypeExprLowering<'_>,
        infer_binders: &crate::semantic_query::InferBinderFactory,
        state: ConditionalLowering<'_>,
        extends_id: SemanticNodeId,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
    ) -> SemanticNodeId {
        let TypeExpr::Conditional {
            check,
            true_type,
            false_type,
            ..
        } = state.expr
        else {
            unreachable!()
        };
        let graph = self.graph();
        let env = lowering.env;
        let scope = lowering.scope;
        let name_resolution = lowering.name_resolution;
        let scope_payload = lowering.scope_payload;
        let shadowing = lowering.shadowing;
        let check_id = state.check.expect("conditional check");
        let reduction_context = state.context;
        let declarations = state.declarations;
        let out_of_scope_env = state.out_of_scope.as_deref();
        let mut lowered = [None, None];
        let output = self.build_conditional_from_lowering(
            check_id,
            extends_id,
            // A naked type parameter as the check type distributes
            // it, an `infer` placeholder of an enclosing conditional
            // among them (`X extends infer K ? K extends "a" ? 1 : 0
            // : 2` distributes over the `K` it binds).
            matches!(check.as_ref(), TypeExpr::TypeParameter(_))
                || matches!(
                    graph.node_data(check_id).as_deref(),
                    Some(SemanticNodeData::TypeParam { .. } | SemanticNodeData::InferRef { .. })
                ),
            &mut |take_true| {
                let index = usize::from(take_true);
                if let Some(node) = lowered[index] {
                    return node;
                }
                if self.cancellation.is_cancelled() {
                    return self.opaque(QueryError::Miss);
                }
                let mut true_env;
                let branch_env = if take_true && !declarations.is_empty() {
                    true_env = env.clone();
                    for (name, binder) in &declarations {
                        let reference = graph.intern_node_with_scope(
                            SemanticNodeData::InferRef {
                                name: OwnedStr::from(name),
                                binder: binder.clone(),
                            },
                            scope.clone(),
                        );
                        true_env.insert(name.to_string(), reference);
                    }
                    &true_env
                } else if take_true {
                    env
                } else {
                    out_of_scope_env.unwrap_or(env)
                };
                let node = self.lower_type_expr_with_infer_factory(
                    infer_binders,
                    if take_true { true_type } else { false_type },
                    branch_env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    reduction_context,
                );
                lowered[index] = Some(node);
                node
            },
        );
        self.deposit_operand_self_roots(&output.observed_self_roots);
        self.fold_into_top_build_local_taint_with(
            output.result_is_partial(),
            output.cache_suppress,
            output.partial_reasons(),
        );
        match output.result {
            QueryResult::Value(node) => node,
            _ => self.opaque(QueryError::Miss),
        }
    }

    fn begin_mapped_lowering<'e>(
        &self,
        lowering: &TypeExprLowering<'_>,
        expr: &'e TypeExpr,
        reduction_context: ProjectionReductionContext,
        frames: &mut LowerFrames<'e>,
    ) -> LowerStep<'e> {
        let TypeExpr::Mapped {
            parameter,
            source,
            value,
            optional,
            readonly,
            name_type,
        } = expr
        else {
            unreachable!()
        };
        let graph = self.graph();
        let env = lowering.env;
        let scope = lowering.scope;
        let mapper_display_name: Arc<str> = Arc::from(parameter.as_str());
        // The shell's structural identity distinguishes this mapper binder
        // from declaration binders in the same file and owner.
        let mapper_decl_name = crate::mapper_binder_registry::mapper_binder_decl_name(
            graph.store(),
            source,
            value,
            *optional,
            *readonly,
            name_type.as_ref(),
        );
        let mapper_decl = match scope {
            NodeScopeId::Global => DeclIdentity {
                canonical_id: verter_type_expr::OwnedStr::from(""),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                whole_hash: HashValue::default(),
                decl_name: verter_type_expr::OwnedStr::from(mapper_decl_name.clone()),
            },
            NodeScopeId::File {
                canonical_id,
                owner,
                whole_hash,
                ..
            } => DeclIdentity {
                canonical_id: canonical_id.clone(),
                owner: *owner,
                whole_hash: *whole_hash,
                decl_name: verter_type_expr::OwnedStr::from(mapper_decl_name.clone()),
            },
        };
        // The host registry assigns the same ordinal to the same source
        // mapper across dispatchers, and distinct ordinals to distinct
        // structural fingerprints under one canonical/display-name pair.
        let fingerprint = crate::mapper_binder_registry::MapperFingerprint::from_components(
            source,
            value,
            *optional,
            *readonly,
            name_type.as_ref(),
        );
        let mapper_ordinal = self.binding.mapper_binders.as_ref().ordinal_for(
            &mapper_decl.canonical_id,
            &mapper_display_name,
            fingerprint,
        );
        // Different ordinals under one display name may reflect substitution
        // fanout as well as distinct source pointers; record that distinction
        // beside the request's substitution observations.
        crate::observe_request!(C::Observe => on self.observation.request.as_deref() => |ctx| {
            let hb = mapper_decl.whole_hash;
            let hash_u64 = u64::from_le_bytes([
                hb[0], hb[1], hb[2], hb[3], hb[4], hb[5], hb[6], hb[7],
            ]);
            let identity = crate::request_context::MapperSourceIdentity {
                canonical_id: std::sync::Arc::<str>::from(
                    mapper_decl.canonical_id.as_str(),
                ),
                whole_hash: hash_u64,
                display_name: Arc::clone(&mapper_display_name),
            };
            ctx.classify_mapper_binder_ordinal(identity, mapper_ordinal);
        });
        let parameter_id = graph.intern_node_with_scope(
            SemanticNodeData::TypeParam {
                decl: mapper_decl,
                param_index: mapper_ordinal,
                // Mapper parameters carry no declaration-site
                // constraint or default in TS mapped syntax —
                // the keyspace is expressed via the outer
                // `[K in S]` binding, not via `T extends` on K.
                constraint: None,
                default: None,
                display_name: OwnedStr::from(&mapper_display_name),
            },
            scope.clone(),
        );
        // A `keyof` operand the environment binds names a type
        // parameter even once it lowers to its argument: the mapping
        // stays homomorphic over it. An unbound operand is read off
        // its lowered node below.
        let over_type_variable = match source.as_ref() {
            TypeExpr::KeyOf(inner) => match inner.as_ref() {
                TypeExpr::TypeParameter(param) => env.contains_key(param.name.as_str()),
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } => type_arguments.is_empty() && env.contains_key(name.as_ref()),
                _ => false,
            },
            _ => false,
        };

        let state = MappedLowering {
            expr,
            context: reduction_context,
            parameter_id,
            over_type_variable,
            source: None,
            key_space: None,
            value: None,
            environment: None,
        };
        frames.push(LowerFrame::Mapped(state, MappedAwait::Source));
        LowerStep::Descend(
            match source.as_ref() {
                TypeExpr::KeyOf(inner) => inner,
                _ => source,
            },
            reduction_context,
        )
    }

    fn resume_mapped_lowering<'e>(
        &self,
        lowering: &TypeExprLowering<'_>,
        mut state: MappedLowering<'e>,
        awaiting: MappedAwait,
        value: SemanticNodeId,
        frames: &mut LowerFrames<'e>,
    ) -> LowerStep<'e> {
        let TypeExpr::Mapped {
            source, name_type, ..
        } = state.expr
        else {
            unreachable!()
        };
        match awaiting {
            MappedAwait::Source => {
                state.source = Some(value);
                if matches!(source.as_ref(), TypeExpr::KeyOf(_)) {
                    let graph = self.graph();
                    if matches!(
                        graph.node_data(value).as_deref(),
                        Some(SemanticNodeData::Infer { .. })
                    ) {
                        state.key_space = Some(graph.intern_node_with_scope(
                            SemanticNodeData::KeyOf { base: value },
                            lowering.scope.clone(),
                        ));
                    } else if crate::semantic_query::may_reduce_operator(state.context) {
                        let key = SemanticQueryKey::KeyOf {
                            base: value,
                            context: state.context,
                        };
                        frames.push(LowerFrame::Read(LowerReadResume::MappedKeySpace(state)));
                        return LowerStep::Read(LowerRead {
                            key,
                            shape: LowerReadShape::Node,
                        });
                    } else {
                        state.key_space = Some(match graph.node_data(value).as_deref() {
                            None => self.opaque(QueryError::Miss),
                            _ if super::raise::node_is_unknown_materializing_failure(
                                self, value,
                            ) =>
                            {
                                self.opaque(QueryError::Miss)
                            }
                            _ => graph.intern_node_with_scope(
                                SemanticNodeData::KeyOf { base: value },
                                lowering.scope.clone(),
                            ),
                        });
                    }
                } else {
                    state.key_space = Some(value);
                }
                self.mapped_value_step(lowering, state, frames)
            }
            MappedAwait::Value => {
                state.value = Some(value);
                if let Some(name) = name_type.as_deref() {
                    let context = state
                        .context
                        .into_structural_transit_with_mode(ProjectionMode::Navigate);
                    let env = Arc::clone(state.environment.as_ref().expect("mapped environment"));
                    frames.push(LowerFrame::Mapped(state, MappedAwait::Name));
                    LowerStep::Scoped(name, context, env)
                } else {
                    self.finish_mapped_lowering(lowering, state, None, frames)
                }
            }
            MappedAwait::Name => self.finish_mapped_lowering(lowering, state, Some(value), frames),
        }
    }

    fn mapped_value_step<'e>(
        &self,
        lowering: &TypeExprLowering<'_>,
        mut state: MappedLowering<'e>,
        frames: &mut LowerFrames<'e>,
    ) -> LowerStep<'e> {
        let TypeExpr::Mapped {
            parameter,
            source,
            value,
            ..
        } = state.expr
        else {
            unreachable!()
        };
        let mut env = lowering.env.clone();
        let graph = self.graph();
        // Only a syntactic `keyof` source introduces its lowered infer binding.
        if matches!(source.as_ref(), TypeExpr::KeyOf(_)) {
            if let Some(SemanticNodeData::Infer { name, binder, .. }) = graph
                .node_data(state.source.expect("mapped source"))
                .as_deref()
            {
                let reference = graph.intern_node_with_scope(
                    SemanticNodeData::InferRef {
                        name: name.clone(),
                        binder: binder.clone(),
                    },
                    lowering.scope.clone(),
                );
                env.insert(name.to_string(), reference);
            }
        }
        env.insert(parameter.clone(), state.parameter_id);
        let env = Arc::new(env);
        state.environment = Some(Arc::clone(&env));
        let context = state
            .context
            .into_structural_transit_with_mode(ProjectionMode::Navigate);
        frames.push(LowerFrame::Mapped(state, MappedAwait::Value));
        LowerStep::Scoped(value, context, env)
    }

    fn finish_mapped_lowering<'e>(
        &self,
        lowering: &TypeExprLowering<'_>,
        state: MappedLowering<'e>,
        name_remap: Option<SemanticNodeId>,
        frames: &mut LowerFrames<'e>,
    ) -> LowerStep<'e> {
        use crate::semantic_query::{MapperKey, OptionalityMod, ReadonlyMod};
        use verter_type_expr::MappedModifier;
        let TypeExpr::Mapped {
            source,
            optional,
            readonly,
            ..
        } = state.expr
        else {
            unreachable!()
        };
        let graph = self.graph();
        let scope = lowering.scope;
        let reduction_context = state.context;
        let source_sem = state.source.expect("mapped source");
        let key_space_sem = state.key_space.expect("mapped key space");
        let value_sem = state.value.expect("mapped value");
        let parameter_id = state.parameter_id;
        let over_type_variable = state.over_type_variable;
        let optionality = match optional {
            MappedModifier::Add => OptionalityMod::Add,
            MappedModifier::Remove => OptionalityMod::Remove,
            MappedModifier::None => OptionalityMod::Keep,
        };
        let readonly_mod = match readonly {
            MappedModifier::Add => ReadonlyMod::Add,
            MappedModifier::Remove => ReadonlyMod::Remove,
            MappedModifier::None => ReadonlyMod::Keep,
        };
        // Classify `value_expr` once at lowering time so
        // `build_mapped_type` matches on `mapper.kind`
        // directly instead of re-inspecting the runtime AST
        // shape on every call. Classification compares the
        // indexed-access index node id against the mapper's
        // binder node id directly, avoiding display-name
        // conflation.
        let kind = crate::semantic_query::MapperKind::classify_value_expr(
            graph.store(),
            value_sem,
            source_sem,
            parameter_id,
        );
        let mapper = MapperKey {
            // The mapper carries the binder's interned
            // `TypeParam` node id, not the display-name
            // string — binder identity in the semantic graph
            // is by `SemanticNodeId`, not by display name.
            parameter_node: parameter_id,
            key_space: key_space_sem,
            value_expr: value_sem,
            optionality,
            readonly: readonly_mod,
            name_remap,
            kind,
            over_type_variable: over_type_variable
                || (matches!(source.as_ref(), TypeExpr::KeyOf(_))
                    && crate::semantic_query::keyof_operand_is_type_variable(
                        graph.store(),
                        source_sem,
                    )),
            modifiers_type: match source.as_ref() {
                TypeExpr::KeyOf(_) => None,
                _ => crate::semantic_query::modifiers_type_of_key_parameter(
                    graph.store(),
                    source_sem,
                ),
            },
        };

        // Preserve an open mapped carrier in every mode. Closed surfaces
        // reduce through the shared MappedType query at the caller's demand.
        if !crate::semantic_query::may_reduce_operator(reduction_context)
            || crate::project_semantic_dispatch::raise::mapped_type_is_open_or_unknown(
                self, source_sem, &mapper,
            )
        {
            return LowerStep::Value(graph.intern_node_with_scope(
                SemanticNodeData::Mapped {
                    source: source_sem,
                    mapper,
                },
                scope.clone(),
            ));
        }

        frames.push(LowerFrame::Read(LowerReadResume::KeyOf));
        LowerStep::Read(LowerRead {
            key: SemanticQueryKey::MappedType {
                source: source_sem,
                mapper,
                context: reduction_context,
            },
            shape: LowerReadShape::Node,
        })
    }

    fn begin_function_lowering<'e>(
        &self,
        lowering: &TypeExprLowering<'_>,
        expr: &'e TypeExpr,
        func: &'e FunctionExpr,
        context: ProjectionReductionContext,
        frames: &mut LowerFrames<'e>,
    ) -> LowerStep<'e> {
        let graph = self.graph();
        let mut state = FunctionLowering {
            func,
            kind: if matches!(expr, TypeExpr::ConstructorType(_)) {
                crate::semantic_query::SignatureKind::Construct
            } else {
                crate::semantic_query::SignatureKind::Call
            },
            context,
            scoped: (!func.type_parameters.is_empty()).then(|| Arc::new(lowering.env.clone())),
            clause: (!func.type_parameters.is_empty() && func.flow_return.is_none()).then(|| {
                crate::mapper_binder_registry::clause_binder_decl_name(graph.store(), expr)
            }),
            shells: Vec::with_capacity(func.type_parameters.len()),
            params: Vec::with_capacity(func.parameters.len()),
            type_parameters: Vec::with_capacity(func.type_parameters.len()),
            constraint: None,
            default: None,
            return_type: None,
            return_carrier: None,
            inferred_predicate: None,
            predicate_target: None,
            stage: FunctionStage::BinderConstraint(0),
            awaiting: FunctionAwait::Return,
        };
        for (ordinal, tp) in func.type_parameters.iter().enumerate() {
            let (decl, param_index) = state.binder_identity(lowering.scope, ordinal);
            state.shells.push(graph.intern_node_with_scope(
                SemanticNodeData::TypeParam {
                    decl,
                    param_index,
                    constraint: None,
                    default: None,
                    display_name: OwnedStr::from(tp.name.as_str()),
                },
                lowering.scope.clone(),
            ));
        }
        self.advance_function_lowering(lowering, state, frames)
    }

    fn resume_function_lowering<'e>(
        &self,
        lowering: &TypeExprLowering<'_>,
        mut state: FunctionLowering<'e>,
        value: SemanticNodeId,
        frames: &mut LowerFrames<'e>,
    ) -> LowerStep<'e> {
        match state.awaiting {
            FunctionAwait::Constraint => state.constraint = Some(value),
            FunctionAwait::Default => state.default = Some(value),
            FunctionAwait::Parameter(index) => {
                let param = &state.func.parameters[index];
                state.params.push(crate::semantic_query::FunctionParam {
                    name: param.name.as_deref().map(OwnedStr::from),
                    ty: value,
                    optional: param.optional,
                    rest: param.rest,
                    span: param.span,
                    declared_literal: crate::semantic_query::declares_literal_type(&param.ty),
                });
            }
            FunctionAwait::Return => {
                state.return_type = Some(value);
                state.return_carrier = Some(
                    crate::semantic_query::SignatureReturnCarrier::Declared(value),
                );
            }
            FunctionAwait::Predicate => state.predicate_target = Some(value),
        }
        self.advance_function_lowering(lowering, state, frames)
    }

    fn advance_function_lowering<'e>(
        &self,
        lowering: &TypeExprLowering<'_>,
        mut state: FunctionLowering<'e>,
        frames: &mut LowerFrames<'e>,
    ) -> LowerStep<'e> {
        let graph = self.graph();
        loop {
            let func = state.func;
            let child = match state.stage {
                FunctionStage::BinderConstraint(i) => {
                    if i == func.type_parameters.len() {
                        state.stage = FunctionStage::Parameter(0);
                        continue;
                    }
                    state.stage = FunctionStage::BinderDefault(i);
                    if let Some(expr) = func.type_parameters[i].constraint.as_deref() {
                        let mut head = state
                            .scoped
                            .as_deref()
                            .expect("generic environment")
                            .clone();
                        for (tp, shell) in func.type_parameters.iter().zip(&state.shells).skip(i) {
                            head.insert(tp.name.clone(), *shell);
                        }
                        state.awaiting = FunctionAwait::Constraint;
                        Some((expr, Some(Arc::new(head))))
                    } else {
                        None
                    }
                }
                FunctionStage::BinderDefault(i) => {
                    state.stage = FunctionStage::Binder(i);
                    state.awaiting = FunctionAwait::Default;
                    func.type_parameters[i]
                        .default
                        .as_deref()
                        .map(|expr| (expr, state.scoped.clone()))
                }
                FunctionStage::Binder(i) => {
                    let tp = &func.type_parameters[i];
                    let (decl, param_index) = state.binder_identity(lowering.scope, i);
                    let binder = graph.intern_node_with_scope(
                        SemanticNodeData::TypeParam {
                            decl,
                            param_index,
                            constraint: state.constraint.take(),
                            default: state.default.take(),
                            display_name: OwnedStr::from(tp.name.as_str()),
                        },
                        lowering.scope.clone(),
                    );
                    Arc::make_mut(state.scoped.as_mut().expect("generic environment"))
                        .insert(tp.name.clone(), binder);
                    state.stage = FunctionStage::BinderConstraint(i + 1);
                    None
                }
                FunctionStage::Parameter(i) => {
                    if i == func.parameters.len() {
                        state.stage = FunctionStage::Return;
                        continue;
                    }
                    state.stage = FunctionStage::Parameter(i + 1);
                    state.awaiting = FunctionAwait::Parameter(i);
                    Some((&func.parameters[i].ty, state.scoped.clone()))
                }
                FunctionStage::Return => {
                    state.stage = FunctionStage::PublishedConstraint(0);
                    if func.flow_return.is_none() {
                        if let Some(expr) = func.return_type.as_deref() {
                            state.awaiting = FunctionAwait::Return;
                            Some((expr, state.scoped.clone()))
                        } else {
                            state.return_type = Some(self.opaque(QueryError::Miss));
                            state.return_carrier =
                                Some(crate::semantic_query::SignatureReturnCarrier::Function(
                                    verter_type_expr::facts::FunctionReturnSource::Absent,
                                ));
                            None
                        }
                    } else {
                        let (return_type, return_carrier, inferred_predicate) = self
                            .lower_function_flow_return(
                                func,
                                state.scoped.as_deref().unwrap_or(lowering.env),
                                lowering.scope,
                            );
                        state.return_type = Some(return_type);
                        state.return_carrier = Some(return_carrier);
                        state.inferred_predicate = inferred_predicate;
                        None
                    }
                }
                FunctionStage::PublishedConstraint(i) => {
                    if i == func.type_parameters.len() {
                        state.stage = FunctionStage::Predicate;
                        continue;
                    }
                    state.stage = FunctionStage::PublishedDefault(i);
                    state.awaiting = FunctionAwait::Constraint;
                    func.type_parameters[i]
                        .constraint
                        .as_deref()
                        .map(|expr| (expr, state.scoped.clone()))
                }
                FunctionStage::PublishedDefault(i) => {
                    state.stage = FunctionStage::Published(i);
                    state.awaiting = FunctionAwait::Default;
                    func.type_parameters[i]
                        .default
                        .as_deref()
                        .map(|expr| (expr, state.scoped.clone()))
                }
                FunctionStage::Published(i) => {
                    let tp = &func.type_parameters[i];
                    state
                        .type_parameters
                        .push(crate::semantic_query::TypeParamDecl {
                            name: OwnedStr::from(tp.name.as_str()),
                            param: *state
                                .scoped
                                .as_ref()
                                .expect("generic environment")
                                .get(tp.name.as_str())
                                .expect("own binder"),
                            constraint: state.constraint.take(),
                            default: state.default.take(),
                            original: None,
                            is_const: tp.is_const,
                        });
                    state.stage = FunctionStage::PublishedConstraint(i + 1);
                    None
                }
                FunctionStage::Predicate => {
                    state.stage = FunctionStage::Finish;
                    state.awaiting = FunctionAwait::Predicate;
                    func.predicate
                        .as_deref()
                        .and_then(|p| p.ty.as_deref())
                        .map(|expr| (expr, state.scoped.clone()))
                }
                FunctionStage::Finish => {
                    let predicate = func
                        .predicate
                        .as_deref()
                        .and_then(|p| {
                            crate::semantic_query::SignaturePredicate::resolve(
                                p,
                                &state.params,
                                state.predicate_target,
                            )
                        })
                        .or(state.inferred_predicate);
                    return LowerStep::Value(graph.intern_node_with_scope(
                        crate::semantic_query::DeferredCallable::seal_signature_header(
                            SemanticNodeData::Signature {
                                kind: state.kind,
                                params: crate::semantic_query::PayloadArray::from(
                                    state.params.into_boxed_slice(),
                                ),
                                return_type: state.return_type.expect("lowered return"),
                                type_parameters: crate::semantic_query::PayloadArray::from(
                                    state.type_parameters.into_boxed_slice(),
                                ),
                                occurrence: None,
                                return_carrier: state.return_carrier.expect("return carrier"),
                                signature_span: func.spans.signature,
                                return_type_span: func.spans.return_type,
                                predicate,
                                is_abstract: func.is_abstract,
                            },
                        ),
                        lowering.scope.clone(),
                    ));
                }
            };
            if let Some((expr, env)) = child {
                let context = state.context;
                frames.push(LowerFrame::Function(state));
                return match env {
                    Some(env) => LowerStep::Scoped(expr, context, env),
                    None => LowerStep::Descend(expr, context),
                };
            }
        }
    }

    fn lower_function_flow_return(
        &self,
        func: &FunctionExpr,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
    ) -> (
        SemanticNodeId,
        crate::semantic_query::SignatureReturnCarrier,
        Option<crate::semantic_query::SignaturePredicate>,
    ) {
        let identity = func
            .flow_return
            .as_ref()
            .expect("body-derived return selected");
        let mut inferred_predicate = None;
        let (node, carrier) = {
            let mut identity = identity.as_ref().clone();
            let scope_canonical = match scope {
                NodeScopeId::File {
                    canonical_id,
                    owner,
                    ..
                } => {
                    identity.anchor.canonical_id = verter_type_expr::OwnedStr::from(canonical_id);
                    identity.anchor.owner = *owner;
                    Some(canonical_id.clone())
                }
                _ => None,
            };
            match scope_canonical {
                Some(scope_canonical) => {
                    let mut carrier = crate::semantic_query::SignatureReturnCarrier::Function(
                        verter_type_expr::facts::FunctionReturnSource::Flow(identity.clone()),
                    );
                    let return_type = match self
                        .probe_signature_flow_return(&identity, scope_canonical.as_ref())
                    {
                        super::flow_return_fact::SignatureReturnProbe::Evaluated(result) => {
                            inferred_predicate = result.inferred_predicate();
                            result.return_type()
                        }
                        super::flow_return_fact::SignatureReturnProbe::Deferred(source) => {
                            carrier = crate::semantic_query::SignatureReturnCarrier::DeferredFlow(
                                source.with_bindings(
                                    env.iter()
                                        .filter(|(name, _)| {
                                            !func
                                                .type_parameters
                                                .iter()
                                                .any(|tp| tp.name == name.as_str())
                                        })
                                        .map(|(name, node)| (Arc::from(name.as_str()), *node)),
                                ),
                            );
                            self.opaque(QueryError::Miss)
                        }
                    };
                    (return_type, carrier)
                }
                None => (
                    self.opaque(QueryError::Miss),
                    crate::semantic_query::SignatureReturnCarrier::Function(
                        verter_type_expr::facts::FunctionReturnSource::Absent,
                    ),
                ),
            }
        };
        (node, carrier, inferred_predicate)
    }

    fn lower_import_arguments<'e>(
        &self,
        continuation: super::carrier::CarrierArgsContinuation,
        arguments: &'e [TypeExpr],
        context: ProjectionReductionContext,
        frames: &mut LowerFrames<'e>,
    ) -> LowerStep<'e> {
        if let Some(first) = arguments.first() {
            frames.push(LowerFrame::RefArguments {
                continuation,
                arguments,
                context,
                lowered: Vec::with_capacity(arguments.len()),
            });
            LowerStep::Descend(first, context)
        } else {
            self.lower_carrier_finish(continuation, Arc::from([]), frames)
        }
    }

    fn lower_ref_arguments<'e>(
        &self,
        continuation: super::carrier::CarrierArgsContinuation,
        type_arguments: &'e [TypeExpr],
        reduction_context: ProjectionReductionContext,
        frames: &mut LowerFrames<'e>,
    ) -> LowerStep<'e> {
        // Type ARGUMENTS lower as CARRIERS, never eagerly.
        //
        // An argument is an operand of the instantiation, and
        // whether it is a LIVE operand is decided by the callee's
        // body — a parameter the body never mentions consumes its
        // argument nowhere. Lowering arguments in the caller's
        // eager mode resolves every argument at the reference
        // site, BEFORE any parameter usage is known, so the cost
        // of `Ignore<A, B> = { only: A }` scales with the
        // structure of the dead `B` argument.
        //
        // Demoting the argument mode to `Navigate` keeps each
        // argument an addressable carrier (`DeclRef` /
        // `InstantiationRef`); a LIVE argument is then forced by
        // the ordinary forcing authority when the substituted
        // body is evaluated under the caller's own context, and a
        // DEAD argument is never forced at all. Identity is
        // unaffected: distinct argument EXPRESSIONS still lower to
        // distinct carriers, so distinct argument environments
        // keep distinct instantiation identities — and the
        // arguments now lower mode-independently, so the same
        // reference reached from a `Navigate` and an `Expanded`
        // demand populates ONE family slot instead of two.
        let context = self.reference_argument_context(
            &continuation,
            reduction_context.into_structural_provenance().with_mode(
                match reduction_context.mode {
                    ProjectionMode::Expanded | ProjectionMode::Identity => ProjectionMode::Navigate,
                    other => other,
                },
            ),
        );
        match type_arguments.first() {
            None => self.lower_carrier_finish(continuation, Arc::from(Vec::new()), frames),
            Some(first) => {
                frames.push(LowerFrame::RefArguments {
                    continuation,
                    arguments: type_arguments,
                    context,
                    lowered: Vec::with_capacity(type_arguments.len()),
                });
                LowerStep::Descend(first, context)
            }
        }
    }

    /// Deliver `value`, the child `frame` descended into, and continue that
    /// node's lowering.
    fn resume_type_expr_frame<'e>(
        &self,
        lowering: &TypeExprLowering<'_>,
        infer_binders: &crate::semantic_query::InferBinderFactory,
        frame: LowerFrame<'e>,
        value: SemanticNodeId,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        frames: &mut LowerFrames<'e>,
    ) -> LowerStep<'e> {
        let graph = self.graph();
        match frame {
            LowerFrame::Environment(_) => unreachable!("environment restored by the cursor"),
            LowerFrame::Function(state) => {
                self.resume_function_lowering(lowering, state, value, frames)
            }
            LowerFrame::Mapped(state, awaiting) => {
                self.resume_mapped_lowering(lowering, state, awaiting, value, frames)
            }
            LowerFrame::Conditional(state, awaiting) => self.resume_conditional_lowering(
                lowering,
                infer_binders,
                (state, awaiting),
                value,
                substitutions,
                frames,
            ),
            LowerFrame::ImportTail(tail) => {
                let key = tail.key(value);
                self.record_carrier_resolution_read_intent(&key);
                // Only the node escapes this lowering; carrier fact status is folded by the read.
                frames.push(LowerFrame::Read(LowerReadResume::ImportTail));
                LowerStep::Read(LowerRead {
                    key,
                    shape: LowerReadShape::Whole,
                })
            }
            LowerFrame::Read(_) => unreachable!("a query frame needs a shaped delivery"),
            LowerFrame::TypeofArguments {
                base,
                arguments,
                context,
                mut lowered,
            } => {
                lowered.push(value);
                match arguments.get(lowered.len()) {
                    Some(next) => {
                        frames.push(LowerFrame::TypeofArguments {
                            base,
                            arguments,
                            context,
                            lowered,
                        });
                        LowerStep::Descend(next, context)
                    }
                    None => LowerStep::Value(self.apply_typeof_instantiation_args(base, &lowered)),
                }
            }

            LowerFrame::RefArguments {
                continuation,
                arguments,
                context,
                mut lowered,
            } => {
                lowered.push(value);
                match arguments.get(lowered.len()) {
                    Some(next) => {
                        frames.push(LowerFrame::RefArguments {
                            continuation,
                            arguments,
                            context,
                            lowered,
                        });
                        LowerStep::Descend(next, context)
                    }
                    None => self.lower_carrier_finish(
                        continuation,
                        Arc::from(lowered.into_boxed_slice()),
                        frames,
                    ),
                }
            }
            LowerFrame::Composite {
                arms,
                union,
                context,
                mut lowered,
            } => {
                lowered.push(value);
                if let Some(next) = arms.get(lowered.len()) {
                    frames.push(LowerFrame::Composite {
                        arms,
                        union,
                        context,
                        lowered,
                    });
                    return LowerStep::Descend(next, context);
                }
                if lowered.len() == 1 {
                    return LowerStep::Value(value);
                }
                // Authored-syntax lowering: the shell keeps authored
                // order and scope; a reduction here is forbidden.
                let arms: Arc<[SemanticNodeId]> = Arc::from(lowered.into_boxed_slice());
                LowerStep::Value(graph.intern_node_with_scope(
                    if union {
                        SemanticNodeData::Union(
                            crate::semantic_query::composite::CompositeList::authored_shell(arms),
                        )
                    } else {
                        SemanticNodeData::Intersection(
                            crate::semantic_query::composite::CompositeList::authored_shell(arms),
                        )
                    },
                    lowering.scope.clone(),
                ))
            }
            LowerFrame::Array { readonly } => LowerStep::Value(graph.intern_node_with_scope(
                SemanticNodeData::Array {
                    element: value,
                    readonly,
                },
                lowering.scope.clone(),
            )),
            LowerFrame::Tuple {
                elements,
                readonly,
                context,
                mut lowered,
            } => {
                let element = &elements[lowered.len()];
                lowered.push(TupleElement {
                    label: element.label.as_deref().map(OwnedStr::from),
                    value,
                    optional: element.optional,
                    rest: element.rest,
                });
                match elements.get(lowered.len()) {
                    Some(next) => {
                        frames.push(LowerFrame::Tuple {
                            elements,
                            readonly,
                            context,
                            lowered,
                        });
                        LowerStep::Descend(&next.ty, context)
                    }
                    None => LowerStep::Value(self.finish_tuple_lowering(
                        lowered,
                        readonly,
                        lowering.scope,
                    )),
                }
            }
            LowerFrame::Template {
                quasis,
                expressions,
                context,
                mut lowered,
            } => {
                lowered.push(value);
                match expressions.get(lowered.len()) {
                    Some(next) => {
                        frames.push(LowerFrame::Template {
                            quasis,
                            expressions,
                            context,
                            lowered,
                        });
                        LowerStep::Descend(next, context)
                    }
                    None => LowerStep::Value(self.finish_template_lowering(
                        quasis,
                        lowered,
                        lowering.scope,
                    )),
                }
            }
            LowerFrame::KeyOf { context } => {
                self.finish_keyof_lowering(value, context, lowering.scope, frames)
            }
            LowerFrame::IndexedObject { index, context } => {
                use crate::semantic_query::IndexKey;
                // Try to reduce literal-string / literal-number indices
                // to a `PathSegment::Index` — fall back to TypeNode for
                // general type-expression indices.
                //
                // G4.4 (bounded): the numeric fold routes through the
                // single shared producer predicate
                // `build::integer_convention_index_key` — a literal
                // becomes `IndexKey::Number(i)` ONLY when `i`'s
                // `Display` IS its canonical `js_number_to_string`
                // spelling, so consumers rendering the needle with
                // `i64::to_string()` are correct by construction.
                // `evaluate::normalized_index_key_node` (and through it
                // the substitution walk's index-key rebuild)
                // applies the same predicate; recovery is the symmetric
                // exact `as f64` raise (`raise::raise_index_key_to_type_expr`,
                // the walker's `Index(Number)` arm). Non-integer
                // literals (`Foo[1.5]`), exponent-regime literals
                // (`Foo[1e21]`), and integral literals whose shortest
                // round-trip diverges from their exact digits
                // (`Foo[4611686018427387904]`) stay `TypeNode`, where
                // the walker's G4.5 recovery re-derives the canonical
                // needle from the literal node.
                let folded_key = match index {
                    TypeExpr::Literal(verter_type_expr::LiteralValue::String(s)) => Some(
                        IndexKey::String(verter_type_expr::OwnedStr::from(s.as_str())),
                    ),
                    TypeExpr::Literal(verter_type_expr::LiteralValue::Number(n)) => {
                        crate::project_semantic_dispatch::build::integer_convention_index_key(*n)
                            .map(IndexKey::Number)
                    }
                    computed @ TypeExpr::TypeOf(_) => {
                        let (node, identity) = self.lower_typeof_for_authored_key(
                            computed,
                            infer_binders,
                            lowering.env,
                            lowering.scope,
                            lowering.name_resolution,
                            lowering.scope_payload,
                            lowering.shadowing,
                            substitutions,
                            context,
                        );
                        Some(
                            identity
                                .map(IndexKey::UniqueSymbol)
                                .unwrap_or(IndexKey::Computed(node)),
                        )
                    }
                    _ => None,
                };
                match folded_key {
                    Some(key) => {
                        self.finish_indexed_access_lowering(value, key, context, lowering, frames)
                    }
                    None => {
                        frames.push(LowerFrame::IndexedIndex {
                            object: value,
                            context,
                        });
                        LowerStep::Descend(index, context)
                    }
                }
            }
            LowerFrame::IndexedIndex { object, context } => self.finish_indexed_access_lowering(
                object,
                crate::semantic_query::IndexKey::Computed(value),
                context,
                lowering,
                frames,
            ),
            LowerFrame::Object {
                object,
                next,
                mut entries,
                context,
                awaiting,
            } => {
                let member_context = context.into_structural_provenance();
                match awaiting {
                    ObjectMemberAwait::PropertyValue(prop) => {
                        // `declared_in_macro_type_arg` reflects the
                        // surface-provenance context: when this object
                        // is lowered directly at the macro
                        // type-argument's own body (an inline
                        // `defineProps<{ a: string }>()` literal, the
                        // directly-referenced declaration's own body
                        // via `build_instantiate`, or an explicit
                        // Object arm of an intersection literal) the
                        // member is author-declared in the macro T.
                        // Otherwise (`Structural`) it is `false`.
                        // This is the canonical typed-IR producer of the
                        // macro-root provenance bit consumed by terminal
                        // projections.
                        entries.push(SurfaceEntry::Member(SurfaceMember {
                            key: self.lower_authored_property_key(
                                &prop.key,
                                infer_binders,
                                lowering.env,
                                lowering.scope,
                                lowering.name_resolution,
                                lowering.scope_payload,
                                lowering.shadowing,
                                substitutions,
                                member_context,
                            ),
                            value,
                            optional: prop.optional,
                            readonly: prop.readonly,
                            method_kind: None,
                            has_implementation_body: false,
                            // Carry the IR member's declared accessibility
                            // verbatim onto the graph payload (Public for
                            // every non-class origin).
                            visibility: prop.visibility,
                            // Carry the IR member's excess-property
                            // provenance verbatim (`FreshOwn` only from
                            // direct object-literal materialization).
                            excess_origin: prop.excess_origin,
                            // Carry the IR member's OXC declaration-site
                            // spans verbatim onto the graph payload.
                            spans: prop.spans,
                            // The member's DECLARATION lives in THIS object's
                            // lowering file — independent of where its value
                            // type resolves (an unresolved `MissingType` value
                            // is scope-less but the member still declares here).
                            declaration_origin: lowering.scope.canonical_file(),
                            declared_in_macro_type_arg: context.own_body_stamp(),
                            // Leaf stamping of the surface-merge role from
                            // the threaded context (by design):
                            // an interface/class own `Object` arm flows
                            // `OwnBody`, a heritage reference arm flows
                            // `Heritage`, everything else stays `Authored`.
                            merge_role: context.role_stamp(),
                        }));
                    }
                    ObjectMemberAwait::IndexKey(sig) => {
                        frames.push(LowerFrame::Object {
                            object,
                            next,
                            entries,
                            context,
                            awaiting: ObjectMemberAwait::IndexValue(sig, value),
                        });
                        return LowerStep::Descend(&sig.value_type, context);
                    }
                    ObjectMemberAwait::IndexValue(sig, key_type) => {
                        entries.push(SurfaceEntry::IndexSignature(IndexSignature {
                            key_type,
                            value_type: value,
                            readonly: sig.readonly,
                            key_name: Some(verter_type_expr::OwnedStr::from(sig.key_name.as_str())),
                            // Carry the IR index signature's OXC spans.
                            spans: sig.spans,
                            // Declaration file of THIS index signature —
                            // from the object's lowering scope, not the
                            // (possibly scope-less) value-type node.
                            declaration_origin: lowering.scope.canonical_file(),
                        }));
                    }
                }
                self.advance_object_lowering(
                    lowering,
                    infer_binders,
                    object,
                    next + 1,
                    entries,
                    context,
                    substitutions,
                    frames,
                )
            }
        }
    }

    /// Continue an object type's member loop at member `next`: a method,
    /// call or construct signature lowers in place as a function type; a
    /// property value or an index signature descends from the explicit
    /// stack. With no member left, the object interns.
    #[allow(clippy::too_many_arguments)]
    fn advance_object_lowering<'e>(
        &self,
        lowering: &TypeExprLowering<'_>,
        infer_binders: &crate::semantic_query::InferBinderFactory,
        object: &'e verter_type_expr::ObjectExpr,
        mut next: usize,
        mut entries: Vec<SurfaceEntry>,
        context: ProjectionReductionContext,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        frames: &mut LowerFrames<'e>,
    ) -> LowerStep<'e> {
        if object.empty_literal && object.properties.is_empty() {
            return LowerStep::Value(self.graph().intern_node_with_scope(
                SemanticNodeData::ObjectSpreadProgram(crate::semantic_query::ObjectSpreadProgram {
                    is_literal: true,
                    effects: crate::semantic_query::PayloadArray::from(
                        Vec::new().into_boxed_slice(),
                    ),
                }),
                lowering.scope.clone(),
            ));
        }
        while let Some(member) = object.properties.get(next) {
            match member {
                ObjectMember::Property(prop) => {
                    frames.push(LowerFrame::Object {
                        object,
                        next,
                        entries,
                        context,
                        awaiting: ObjectMemberAwait::PropertyValue(prop),
                    });
                    // Member VALUE lowering downgrades to
                    // structural provenance (Stage
                    // 1): a nested object inside this member's
                    // type (`{ outer: { inner: T } }`) is NOT the
                    // macro-T own body — only THIS object's
                    // direct members are. Stamping the value with
                    // macro provenance would mis-mark `inner`.
                    return LowerStep::Descend(&prop.ty, context.into_structural_provenance());
                }
                ObjectMember::Method(method) => {
                    // Mapped+conditional infer closure: lower
                    // methods to canonical Function nodes (matching
                    // CallSignature handling below) so
                    // `PricingPlanSlots[K]` IndexedAccess can
                    // resolve to a real Function for the
                    // Function-extends infer-binding arm. An
                    // `Opaque(Miss)` placeholder here would break
                    // `IndexedAccess<I, "method-name">`
                    // projection: the path walker finds the
                    // member but its value is opaque, so the
                    // downstream `let Some(Function...) =
                    // graph.node_data(check_resolved)` match
                    // fails and the conditional drops to a
                    // deferred shell.
                    let function_expr = TypeExpr::Function(Arc::new(method.function.clone()));
                    register_eager_function_alias(infer_binders, &function_expr, &method.function);
                    // Method VALUE (its function shape) lowers
                    // structurally — see the `ObjectMember::Property`
                    // companion note. Only the method's presence on
                    // THIS object is macro-T own-body, not the
                    // function's nested parameter/return objects.
                    let value = self.lower_type_expr_with_infer_factory(
                        infer_binders,
                        &function_expr,
                        lowering.env,
                        lowering.scope,
                        lowering.name_resolution,
                        lowering.scope_payload,
                        lowering.shadowing,
                        substitutions,
                        context.into_structural_provenance(),
                    );
                    // `declared_in_macro_type_arg` mirrors the
                    // `ObjectMember::Property` arm: a method
                    // literally written in the macro type
                    // argument's own body is author-declared.
                    entries.push(SurfaceEntry::Member(SurfaceMember {
                        key: self.lower_authored_property_key(
                            &method.key,
                            infer_binders,
                            lowering.env,
                            lowering.scope,
                            lowering.name_resolution,
                            lowering.scope_payload,
                            lowering.shadowing,
                            substitutions,
                            context.into_structural_provenance(),
                        ),
                        value,
                        optional: method.optional,
                        readonly: false,
                        method_kind: Some(method.method_kind),
                        has_implementation_body: method.has_implementation_body,
                        // Carry the IR method's declared accessibility
                        // (Public for every non-class origin).
                        visibility: method.visibility,
                        // Carry the IR method's excess-property
                        // provenance verbatim.
                        excess_origin: method.excess_origin,
                        // Carry the IR method's OXC member spans.
                        spans: method.spans,
                        // Declaration file of THIS method (see the
                        // `Property` companion note).
                        declaration_origin: lowering.scope.canonical_file(),
                        declared_in_macro_type_arg: context.own_body_stamp(),
                        // Leaf stamping of the surface-merge role —
                        // mirrors the `Property` arm.
                        merge_role: context.role_stamp(),
                    }));
                }
                ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                    // Lower call signatures as canonical `Function`
                    // nodes so utility dispatch (`ReturnType`,
                    // `Parameters`, `InstanceType`,
                    // `ConstructorParameters`, `Awaited`) can
                    // inspect parameter / return structure at the
                    // graph level instead of falling back to an
                    // opaque miss. The reverse mapping
                    // `raise_node_to_type_expr` reconstitutes
                    // `ObjectMember::CallSignature` entries from
                    // `SurfaceView.call_signatures` by matching
                    // `TypeExpr::Function(...)`.
                    let construct = matches!(member, ObjectMember::ConstructSignature(_));
                    let function_expr = if construct {
                        TypeExpr::ConstructorType(Arc::new(func.clone()))
                    } else {
                        TypeExpr::Function(Arc::new(func.clone()))
                    };
                    register_eager_function_alias(infer_binders, &function_expr, func);
                    let fn_id = self.lower_type_expr_with_infer_factory(
                        infer_binders,
                        &function_expr,
                        lowering.env,
                        lowering.scope,
                        lowering.name_resolution,
                        lowering.scope_payload,
                        lowering.shadowing,
                        substitutions,
                        context,
                    );
                    entries.push(if construct {
                        SurfaceEntry::ConstructSignature(fn_id)
                    } else {
                        SurfaceEntry::CallSignature(fn_id)
                    });
                }
                // Unreachable by construction: a spread-bearing object
                // lowers through the spread materializer as a leaf.
                ObjectMember::Spread(_) => {}
                ObjectMember::IndexSignature(sig) => {
                    frames.push(LowerFrame::Object {
                        object,
                        next,
                        entries,
                        context,
                        awaiting: ObjectMemberAwait::IndexKey(sig),
                    });
                    return LowerStep::Descend(&sig.key_type, context);
                }
            }
            next += 1;
        }
        let has_index_signature = entries
            .iter()
            .any(|entry| matches!(entry, SurfaceEntry::IndexSignature(_)));
        let view = SurfaceView::from_entries(entries, None, has_index_signature);
        LowerStep::Value(
            self.graph()
                .intern_node_with_scope(SemanticNodeData::Object(view), lowering.scope.clone()),
        )
    }

    /// A tuple of its lowered elements.
    fn finish_tuple_lowering(
        &self,
        lowered_elements: Vec<TupleElement>,
        readonly: bool,
        scope: &NodeScopeId,
    ) -> SemanticNodeId {
        // Normalize-on-intern (the variadic-spread rule): when an
        // instantiation env already substituted a rest element's
        // binder to a concrete tuple (`[...A, ...B]` lowered with
        // `A = [1, 2]`), the spread splices in place; a sole
        // rest-of-array tuple collapses to the array. Open rest
        // values (unbound generics, carriers) are preserved
        // verbatim — decl-body lowering stays carrier-shaped.
        match self.normalize_tuple_spread(&lowered_elements, readonly) {
            crate::project_semantic_dispatch::build::NormalizedTupleShape::Array(array_node) => {
                array_node
            }
            crate::project_semantic_dispatch::build::NormalizedTupleShape::Tuple(normalized) => {
                self.graph().intern_node_with_scope(
                    SemanticNodeData::Tuple {
                        elements: crate::semantic_query::PayloadArray::from(
                            normalized.into_boxed_slice(),
                        ),
                        readonly,
                    },
                    scope.clone(),
                )
            }
        }
    }

    /// A template literal of its lowered holes.
    fn finish_template_lowering(
        &self,
        quasis: &[String],
        lowered_expressions: Vec<SemanticNodeId>,
        scope: &NodeScopeId,
    ) -> SemanticNodeId {
        let lowered_quasis: Vec<OwnedStr> =
            quasis.iter().map(|q| OwnedStr::from(q.as_str())).collect();
        self.graph().intern_node_with_scope(
            SemanticNodeData::TemplateLiteral {
                quasis: verter_type_expr::OwnedSlice::from(lowered_quasis),
                expressions: crate::semantic_query::PayloadArray::from(
                    lowered_expressions.into_boxed_slice(),
                ),
            },
            scope.clone(),
        )
    }

    /// `keyof` over its lowered operand.
    fn finish_keyof_lowering<'e>(
        &self,
        base_id: SemanticNodeId,
        reduction_context: ProjectionReductionContext,
        scope: &NodeScopeId,
        frames: &mut LowerFrames<'e>,
    ) -> LowerStep<'e> {
        let graph = self.graph();
        if crate::semantic_query::may_reduce_operator(reduction_context) {
            frames.push(LowerFrame::Read(LowerReadResume::KeyOf));
            LowerStep::Read(LowerRead {
                key: SemanticQueryKey::KeyOf {
                    base: base_id,
                    context: reduction_context,
                },
                shape: LowerReadShape::Node,
            })
        } else {
            LowerStep::Value(match graph.node_data(base_id).as_deref() {
                None => self.opaque(QueryError::Miss),
                _ if super::raise::node_is_unknown_materializing_failure(self, base_id) => {
                    self.opaque(QueryError::Miss)
                }
                _ => graph.intern_node_with_scope(
                    SemanticNodeData::KeyOf { base: base_id },
                    scope.clone(),
                ),
            })
        }
    }

    /// An indexed access over its lowered object and key.
    fn finish_indexed_access_lowering<'e>(
        &self,
        obj_id: SemanticNodeId,
        index_key: crate::semantic_query::IndexKey,
        reduction_context: ProjectionReductionContext,
        lowering: &TypeExprLowering<'_>,
        frames: &mut LowerFrames<'e>,
    ) -> LowerStep<'e> {
        let graph = self.graph();
        let scope = lowering.scope;
        // A binding the lowering holds may have gone into the operands, so
        // only an access with none is known to be the one written.
        let origin = if lowering.env.is_empty() {
            crate::semantic_query::IndexedAccessOrigin::Written
        } else {
            crate::semantic_query::IndexedAccessOrigin::Composed
        };
        // Operand lowering preserves the operator. Only its value demand may
        // select the member; a transit read must retain both path segments.
        let should_defer = !crate::semantic_query::may_reduce_operator(reduction_context)
            || reduction_context.mode == ProjectionMode::Skeleton
            || !matches!(
                graph.node_data(obj_id).as_deref(),
                Some(SemanticNodeData::Object(_))
            )
            || self.indexed_access_reads_named_declaration(obj_id, &index_key);
        if should_defer {
            LowerStep::Value(graph.intern_node_with_scope(
                SemanticNodeData::IndexedAccess {
                    object: obj_id,
                    index: index_key,
                    origin,
                },
                scope.clone(),
            ))
        } else {
            // Path-precision rule: the literal `T[K]` single-hop
            // is the TERMINAL projection of THIS indexed access,
            // so it runs in the CALLER's mode (not a hardcoded
            // `Navigate`). Its object was lowered at operand altitude;
            // only this selected terminal can expand.
            // A structural-transit caller keeps transit/Navigate via
            // its own `reduction_context.mode`.
            let key = SemanticQueryKey::IndexedAccess {
                base: obj_id,
                index: index_key.clone(),
                mode: reduction_context.mode,
            };
            frames.push(LowerFrame::Read(LowerReadResume::Indexed {
                object: obj_id,
                index: index_key,
                origin,
                context: reduction_context,
            }));
            LowerStep::Read(LowerRead {
                key,
                shape: LowerReadShape::Node,
            })
        }
    }

    /// Lower one node that is not a structural position of
    /// [`Self::lower_type_expr_with_infer_factory`]'s explicit stack: its
    /// children lower through that entry, one native level beneath this
    /// one.
    #[allow(clippy::too_many_arguments)]
    fn lower_type_expr_leaf(
        &self,
        infer_binders: &crate::semantic_query::InferBinderFactory,
        expr: &TypeExpr,
        env: &FxHashMap<String, SemanticNodeId>,
        scope: &NodeScopeId,
        name_resolution: &FxHashMap<verter_type_expr::OwnedStr, ResolvedRootIdentity>,
        scope_payload: Option<&DeclarationScopePayload>,
        shadowing: &ScopeShadowing,
        substitutions: &mut Vec<(Arc<str>, SemanticNodeId)>,
        reduction_context: ProjectionReductionContext,
    ) -> SemanticNodeId {
        let graph = self.graph();
        match expr {
            TypeExpr::Primitive(name) => graph.intern_node_with_scope(
                SemanticNodeData::Primitive(map_primitive_name(*name)),
                scope.clone(),
            ),
            TypeExpr::Literal(value) => graph
                .intern_node_with_scope(SemanticNodeData::Literal(value.clone()), scope.clone()),
            TypeExpr::TypeParameter(param) => {
                if let Some(arg_id) = env.get(&param.name) {
                    substitutions.push((Arc::from(param.name.as_str()), *arg_id));
                    *arg_id
                } else {
                    // Unbound parameter — intern with lowered
                    // constraint / default so the projection back to
                    // `TypeExpr::TypeParameter(TypeParam { name,
                    // constraint, default })` is complete.
                    let constraint = param.constraint.as_ref().map(|c| {
                        self.lower_type_expr_with_infer_factory(
                            infer_binders,
                            c,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            reduction_context,
                        )
                    });
                    let default = param.default.as_ref().map(|d| {
                        self.lower_type_expr_with_infer_factory(
                            infer_binders,
                            d,
                            env,
                            scope,
                            name_resolution,
                            scope_payload,
                            shadowing,
                            substitutions,
                            reduction_context,
                        )
                    });
                    let display_name = OwnedStr::from(param.name.as_str());
                    // Unresolved `TypeParameter` path uses
                    // **file-scoped name-keyed identity**:
                    // `decl_name = reference.name` (NOT the owning
                    // declaration's name, which is unavailable at
                    // this site because the parameter could not be
                    // resolved). Two unresolved `K` references
                    // anywhere in the same file alias to one
                    // `SemanticNodeId`; cross-file unresolved `K`
                    // references stay distinct via `canonical_id`.
                    // `param_index = 0` is the file-scoped name-keyed
                    // identity slot; the escalation path if this
                    // proves too coarse is an owner-scope-local
                    // `(name → ordinal)` map.
                    let decl = crate::semantic_query::DeclIdentity::from_scope(
                        scope,
                        display_name.clone(),
                    );
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
            }
            // `type Foo<T> = { x: T }` — the parser keeps bare `T` as
            // `TypeExpr::Ref { name: "T", type_arguments: [] }` at top-level
            // alias bodies (only function-type parameters are normalised
            // via `normalize_type_parameter_refs`). Check the
            // substitution env first; a named match means this is a
            // parameter reference that should substitute.
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() && env.contains_key(name.as_ref()) => {
                let arg_id = env.get(name.as_ref()).copied().unwrap();
                substitutions.push((Arc::clone(name), arg_id));
                arg_id
            }
            // Script-setup generic parameter. When the bare name maps
            // to a `script_setup_type_bindings` entry, lower directly
            // to a rich
            // `SemanticNodeData::TypeParam { name, constraint, default }`
            // — NOT via the `ResolveDecl` fallback. This preserves
            // declaration-site constraint/default so the projection
            // back to `TypeExpr::TypeParameter(TypeParam)` is complete
            // at meta-extraction time. Must match on
            // `scope_type_bindings` specifically (the script-setup
            // map), not `scope_type_names` which also contains
            // same-file type decls.
            //
            // The binding store is
            // [`verter_session_query::inputs::prepared::TypeParamBinding`]
            // (the content-free name + ordinal fact pair); the ONE
            // shared helper re-borrows the clause lease-only from the
            // pinned artifact and constructs the node — never an
            // intermediate `PreparedTypeDecl` wrapper.
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty()
                && scope_payload
                    .map(|payload| payload.scope_type_bindings().contains_key(name.as_ref()))
                    .unwrap_or(false) =>
            {
                let binding = scope_payload
                    .and_then(|payload| payload.scope_type_bindings().get(name.as_ref()))
                    .expect("matched on scope_type_bindings.contains_key above");
                self.lower_script_setup_type_param_binding(
                    binding,
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    reduction_context,
                )
            }
            // `infer X` placeholder in a conditional's `extends` arm
            // Explicit semantic variant rather
            // than encoded via scope overloading. Substitution picks
            // the Infer arm up symmetrically with TypeParam in
            // `substitute_semantic_type_param`; `build_conditional`
            // recognises a bare Infer in `extends` and binds the
            // true-branch's placeholder to the check side.
            // An `infer` no conditional's `extends` declares (the checker
            // reports it) reads no constraint.
            TypeExpr::Infer { name, .. } => {
                if let Some(declaration) = env.get(&infer_declaration_env_key(name)) {
                    *declaration
                } else {
                    graph.intern_node_with_scope(
                        SemanticNodeData::Infer {
                            name: OwnedStr::from(name.as_str()),
                            binder: infer_binders.binder_for_expr(expr),
                            constraint: None,
                        },
                        scope.clone(),
                    )
                }
            }
            // A spread-bearing object literal folds through the shared
            // spread materializer (ordered left fold over direct members
            // and spread operands).
            TypeExpr::Object(obj)
                if obj
                    .properties
                    .iter()
                    .any(|m| matches!(m, ObjectMember::Spread(_))) =>
            {
                self.lower_spread_object_literal(
                    obj,
                    infer_binders,
                    env,
                    scope,
                    name_resolution,
                    scope_payload,
                    shadowing,
                    substitutions,
                    reduction_context,
                )
            }
            // Every other structural position lowers from the explicit stack.
            TypeExpr::Mapped { .. }
            | TypeExpr::Conditional { .. }
            | TypeExpr::Function(_)
            | TypeExpr::ConstructorType(_)
            | TypeExpr::ImportType { .. }
            | TypeExpr::Ref { .. }
            | TypeExpr::Union(_)
            | TypeExpr::Intersection(_)
            | TypeExpr::Array { .. }
            | TypeExpr::Tuple { .. }
            | TypeExpr::TemplateLiteral { .. }
            | TypeExpr::Parenthesized(_)
            | TypeExpr::KeyOf(_)
            | TypeExpr::IndexedAccess { .. }
            | TypeExpr::Object(_) => self.lower_type_expr_with_infer_factory(
                infer_binders,
                expr,
                env,
                scope,
                name_resolution,
                scope_payload,
                shadowing,
                substitutions,
                reduction_context,
            ),
            // Unsupported leaf forms keep the typed miss.
            _ => self.opaque(QueryError::Miss),
        }
    }
}
