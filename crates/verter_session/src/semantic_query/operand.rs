//! Closed carrier for demand-selected semantic inputs.
//!
//! Operands are minted only by [`ProjectSemanticDispatch`](crate::project_semantic_dispatch::ProjectSemanticDispatch).
//! Authored identity is content-free and seals the exact locator, lexical
//! scope, declaration binder, substitution, and all five host environment
//! dimensions. Runtime nodes additionally carry their issuing store and
//! generation. Projection policy belongs exclusively to the one-shot force
//! request.

// The operand vocabulary's only consumer is the forcing boundary in
// `project_semantic_dispatch::semantic_operand` and its co-located tests,
// so a plain library build sees the closed carrier as unreachable. The
// suppression is scoped to `not(test)`
// deliberately: under `cfg(test)` — the configuration
// `clippy --all-targets` compiles — dead-code analysis stays ARMED, so an
// item that no production path AND no test exercises still surfaces as a
// genuine orphan rather than hiding behind a blanket allow.
#![cfg_attr(not(test), allow(dead_code))]

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use verter_type_expr::locators::{
    AuthoredBodyLocator, LocatorSymbolSpace, TypeBodyPathStep, TypeParamBoundPosition,
};
use verter_type_expr::TopLevelOwnerId;

use super::{IndexKey, PathSegment, ProjectionReductionContext, PropertyKey, SemanticNodeId};
use crate::locator_identity::{
    LibEnvHash, ParseEnvHash, ProjectIdentityDim, ResolveEnvHash, TypeEnvHash,
};
use crate::project_semantic_dispatch::SemanticOperandAuthority;

/// Typed refusal while sealing an operand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticOperandMintError {
    /// The anchor declaration exists, but in the other symbol space than
    /// the locator names — sealing it would lower a value declaration under
    /// a type-space binder frame (or the reverse).
    WrongAnchorSpace {
        expected: LocatorSymbolSpace,
        actual: LocatorSymbolSpace,
    },
    /// The locator's anchor declaration is absent from the owning file in
    /// either symbol space, so no authored binder frame exists to seal.
    MissingAuthoredDeclaration,
    /// The locator's path addresses an authored position the anchor
    /// declaration does not declare (for example a value-signature
    /// ordinal past the declaration's overload group). Sealing it would
    /// defer an already-decidable refusal to an anonymous body-deref
    /// miss at force time.
    UnresolvedLocatorPath,
    UnstableEnvironment,
    /// More substitution arguments were supplied than the anchor
    /// declaration's header declares (`expected` = the declared count,
    /// `actual` = the supplied count). Supplying FEWER is legal — the
    /// unsupplied parameters fall back to their own declared defaults, or
    /// stay unbound shells — but a surplus argument binds nothing while
    /// still entering the family key, so admitting it would fragment the
    /// cache across keys holding identical values.
    SubstitutionArity {
        expected: usize,
        actual: usize,
    },
    /// The locator addresses the bound of a header type parameter the
    /// anchor declaration does not declare. Deferring this to force time
    /// would surface as a generic body-deref miss rather than naming the
    /// out-of-range frame.
    BoundOrdinalOutOfRange {
        ordinal: u32,
        declared: usize,
    },
    /// A substitution argument was an `Authored` (not-yet-forced) operand —
    /// distinct from [`Self::ForeignNode`], which is a forced node from the
    /// wrong store/generation.
    UnboundSubstitution,
    ForeignNode,
    SignatureOverflow,
}

impl std::fmt::Display for SemanticOperandMintError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Content-free lexical identity of the file scope an authored operand's
/// selection resolves names in.
///
/// The AUTHORITY derives it from the locator's anchor; a caller can never
/// supply it, which is the whole point of a sealed operand — an operand
/// whose scope could be handed in separately from its locator could name a
/// declaration in one file and resolve its free references in another. The
/// two can therefore never disagree, and the carrier is a NAMED axis of
/// the sealed identity rather than an independent discriminator: it is
/// hashed alongside the locator so the family key states the scope
/// explicitly instead of leaving it implicit in a structural field.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OperandLexicalScope {
    canonical_id: Arc<str>,
    owner: TopLevelOwnerId,
}

impl OperandLexicalScope {
    /// Authority-side derivation from the locator's anchor. Private: a
    /// caller can never supply or reconstruct the scope independently,
    /// which is the whole point of a sealed operand.
    fn for_locator(locator: &AuthoredBodyLocator) -> Self {
        let anchor = authored_anchor(locator);
        Self {
            canonical_id: Arc::clone(&anchor.canonical_id),
            owner: anchor.owner,
        }
    }
}

/// Which declaration-header binder frame an authored position lowers
/// under. TypeScript's lexical visibility differs per position: a
/// constraint may reference every sibling parameter and itself, a default
/// may reference prior siblings only (later siblings are present as shadow
/// entries but forbidden), and the body sees the final frame. Two
/// selections at the same declaration under different frames are different
/// binder scopes even though they share an anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperandBinderVisibility {
    /// The declaration body's final frame.
    Body,
    /// The `ordinal`-th parameter's constraint frame (`T extends C`).
    Constraint { ordinal: u32 },
    /// The `ordinal`-th parameter's default frame (`T = D`).
    Default { ordinal: u32 },
}

/// Content-free identity of the binder scope an authored operand's
/// selection is lowered inside.
///
/// The binder scope of an authored selection is fixed by three things, and
/// this carrier holds the one of them the locator does not already state:
///
/// - the declaration whose header frame is in scope — carried by
///   [`OperandLexicalScope`] and the locator's anchor;
/// - the *frame* of that declaration the position sees — this carrier's
///   [`OperandBinderVisibility`], derived by the sealing authority from the
///   locator, never supplied by a caller;
/// - the *instantiation* of that frame — the operand's `substitution`,
///   which is a separate sealed axis, so two forces that land on the same
///   authored `Mapped`/`Conditional` binder position through different
///   enclosing substitutions never share a family entry.
///
/// Runtime binder handles (`NodeScopeId`, `InferBinderId`, a mapper binder)
/// are store- and generation-local, so they are identity only on the
/// runtime-node arm of [`SemanticOperand`] and never on the authored arm,
/// whose whole point is to survive independently of any one graph.
///
/// The carrier is deliberately edit-stable: it names positions, not the
/// parameter spellings at those positions, so renaming `T` to `U` in the
/// anchor declaration invalidates through the operand's read set rather
/// than by minting a new family key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperandBinderIdentity {
    visibility: OperandBinderVisibility,
}

impl OperandBinderIdentity {
    /// Classify the frame `locator` selects. A leading
    /// [`TypeBodyPathStep::TypeParamBound`] addresses a header parameter's
    /// bound (the only positions with a non-body frame); every other path
    /// lands in the declaration body.
    pub(crate) fn for_locator(locator: &AuthoredBodyLocator) -> Self {
        let path = match locator {
            AuthoredBodyLocator::DeclBody(slot) => slot.path.as_ref(),
            AuthoredBodyLocator::AugmentationBody(body) => body.path.as_ref(),
            AuthoredBodyLocator::JsdocTypedefBody(body) => body.path.as_ref(),
            AuthoredBodyLocator::MacroPayload(_) => &[],
        };
        let visibility = match path.first() {
            Some(TypeBodyPathStep::TypeParamBound {
                ordinal,
                position: TypeParamBoundPosition::Constraint,
            }) => OperandBinderVisibility::Constraint { ordinal: *ordinal },
            Some(TypeBodyPathStep::TypeParamBound {
                ordinal,
                position: TypeParamBoundPosition::Default,
            }) => OperandBinderVisibility::Default { ordinal: *ordinal },
            _ => OperandBinderVisibility::Body,
        };
        Self { visibility }
    }

    /// The header parameter ordinal this frame is the BOUND of, or `None`
    /// for a body frame.
    ///
    /// This is the axis's load-bearing read: the sealing authority checks
    /// it against the anchor declaration's declared header count, so a
    /// locator naming a bound of a parameter that does not exist is a
    /// typed refusal AT THE SEAL
    /// ([`SemanticOperandMintError::BoundOrdinalOutOfRange`]) rather than a
    /// deferred body-deref miss. A derivation that collapsed every
    /// position onto [`OperandBinderVisibility::Body`] would silently seal
    /// those operands.
    #[must_use]
    pub(crate) const fn bound_ordinal(self) -> Option<u32> {
        match self.visibility {
            OperandBinderVisibility::Body => None,
            OperandBinderVisibility::Constraint { ordinal }
            | OperandBinderVisibility::Default { ordinal } => Some(ordinal),
        }
    }
}

/// Atomic five-way environment snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperandSplitEnv {
    parse_env_hash: ParseEnvHash,
    resolve_env_hash: ResolveEnvHash,
    type_env_hash: TypeEnvHash,
    lib_env_hash: LibEnvHash,
    project_identity: ProjectIdentityDim,
}

impl OperandSplitEnv {
    /// Token-gated construction: the forcing authority snapshots all five
    /// dimensions as one atomic observation; without the unforgeable
    /// [`SemanticOperandAuthority`] no consumer can mint an environment
    /// tuple for an operand.
    pub(crate) const fn new(
        parse_env_hash: ParseEnvHash,
        resolve_env_hash: ResolveEnvHash,
        type_env_hash: TypeEnvHash,
        lib_env_hash: LibEnvHash,
        project_identity: ProjectIdentityDim,
        _authority: SemanticOperandAuthority,
    ) -> Self {
        Self {
            parse_env_hash,
            resolve_env_hash,
            type_env_hash,
            lib_env_hash,
            project_identity,
        }
    }

    /// Destructure into the five axes. Reachable only for a tuple the
    /// token-gated constructor (or the sealed operand read path) produced.
    pub(crate) const fn parts(
        self,
    ) -> (
        ParseEnvHash,
        ResolveEnvHash,
        TypeEnvHash,
        LibEnvHash,
        ProjectIdentityDim,
    ) {
        (
            self.parse_env_hash,
            self.resolve_env_hash,
            self.type_env_hash,
            self.lib_env_hash,
            self.project_identity,
        )
    }
}

/// Non-identity runtime evidence retained with a store-local node handle.
///
/// The type lives with its producer: the graph store
/// (`semantic_query_memo`) owns the fields
/// (`pub(in crate::semantic_query_memo)` — the store's capture path is the
/// ONE struct-literal producer), and the forcing authority combines
/// evidence only through the token-gated
/// [`SemanticOperandEvidence::seal`]. No other internal consumer can
/// fabricate an evidence set and hand it to a mint. Re-exported here so
/// the operand vocabulary keeps one name for it.
pub(crate) use crate::semantic_query_memo::SemanticOperandEvidence;

#[derive(Debug, Clone)]
pub(crate) struct AuthoredSemanticOperand {
    locator: AuthoredBodyLocator,
    lexical_scope: OperandLexicalScope,
    binder: OperandBinderIdentity,
    substitution: Arc<[SemanticNodeId]>,
    split_env: OperandSplitEnv,
    substitution_evidence: SemanticOperandEvidence,
    substitution_runtime: Option<(u64, u64)>,
}

/// Content-free authored axes carried into the context-bearing query family.
///
/// Construction is authority-side ([`AuthoredSemanticOperand::query_identity`],
/// reachable only through the token-gated [`SemanticOperand::parts`]); the
/// memo family stores the identity opaquely behind derived `Hash`/`Eq` and
/// can never read the sealed axes back out.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct AuthoredOperandQueryIdentity {
    locator: AuthoredBodyLocator,
    lexical_scope: OperandLexicalScope,
    binder: OperandBinderIdentity,
    split_env: OperandSplitEnv,
}

impl AuthoredOperandQueryIdentity {
    /// The sealed authored locator. Read by the forcing authority's own
    /// build path (`build_authored_instantiation`).
    pub(crate) fn locator(&self) -> &AuthoredBodyLocator {
        &self.locator
    }
}

impl AuthoredSemanticOperand {
    /// Seal an authored locator. Lexical scope and binder frame are derived
    /// from the locator; callers cannot supply them independently.
    fn from_authority(
        locator: AuthoredBodyLocator,
        substitution: Arc<[SemanticNodeId]>,
        split_env: OperandSplitEnv,
        substitution_evidence: SemanticOperandEvidence,
        substitution_runtime: Option<(u64, u64)>,
    ) -> Self {
        Self {
            lexical_scope: OperandLexicalScope::for_locator(&locator),
            binder: OperandBinderIdentity::for_locator(&locator),
            locator,
            substitution,
            split_env,
            substitution_evidence,
            substitution_runtime,
        }
    }

    pub(crate) fn locator(&self) -> &AuthoredBodyLocator {
        &self.locator
    }

    pub(crate) fn substitution(&self) -> &Arc<[SemanticNodeId]> {
        &self.substitution
    }

    pub(crate) fn split_env(&self) -> OperandSplitEnv {
        self.split_env
    }

    pub(crate) fn substitution_evidence(&self) -> &SemanticOperandEvidence {
        &self.substitution_evidence
    }

    pub(crate) fn substitution_runtime(&self) -> Option<(u64, u64)> {
        self.substitution_runtime
    }

    pub(crate) fn query_identity(&self) -> AuthoredOperandQueryIdentity {
        AuthoredOperandQueryIdentity {
            locator: self.locator.clone(),
            lexical_scope: self.lexical_scope.clone(),
            binder: self.binder,
            split_env: self.split_env,
        }
    }

    /// Whether the locator addresses the WHOLE declaration body of a
    /// type-space declaration (an empty path implies the body frame):
    /// the one authored position whose answer IS the declaration-source
    /// `Instantiate` query's, so the force can converge on that family
    /// instead of forking an authored-source duplicate.
    pub(crate) fn addresses_whole_type_declaration(&self) -> bool {
        matches!(&self.locator, AuthoredBodyLocator::DeclBody(slot) if slot.path.is_empty())
            && authored_anchor(&self.locator).space == LocatorSymbolSpace::Type
    }
}

#[derive(Debug, Clone)]
enum SemanticOperandKind {
    Node {
        store_identity: u64,
        generation: u64,
        node: SemanticNodeId,
        evidence: SemanticOperandEvidence,
    },
    Authored(Box<AuthoredSemanticOperand>),
}

/// Closed semantic operand. Only runtime nodes are store/generation qualified.
#[derive(Debug, Clone)]
pub struct SemanticOperand {
    kind: SemanticOperandKind,
}

impl SemanticOperand {
    /// Token-gated construction of the runtime-node arm: the forcing
    /// authority seals a published node to its store and generation.
    pub(crate) fn node(
        store_identity: u64,
        generation: u64,
        node: SemanticNodeId,
        evidence: SemanticOperandEvidence,
        _authority: SemanticOperandAuthority,
    ) -> Self {
        Self {
            kind: SemanticOperandKind::Node {
                store_identity,
                generation,
                node,
                evidence,
            },
        }
    }

    fn authored(authored: AuthoredSemanticOperand) -> Self {
        Self {
            kind: SemanticOperandKind::Authored(Box::new(authored)),
        }
    }

    /// The only production construction path for an authored operand.
    /// Scope and binder are derived from `locator`. Token-gated: without
    /// the unforgeable [`SemanticOperandAuthority`] no internal consumer
    /// can mint an authored operand.
    pub(crate) fn from_authored_authority(
        locator: AuthoredBodyLocator,
        substitution: Arc<[SemanticNodeId]>,
        split_env: OperandSplitEnv,
        substitution_evidence: SemanticOperandEvidence,
        substitution_runtime: Option<(u64, u64)>,
        _authority: SemanticOperandAuthority,
    ) -> Self {
        Self::authored(AuthoredSemanticOperand::from_authority(
            locator,
            substitution,
            split_env,
            substitution_evidence,
            substitution_runtime,
        ))
    }

    #[cfg(test)]
    pub(crate) fn with_split_env(
        &self,
        split_env: OperandSplitEnv,
        authority: SemanticOperandAuthority,
    ) -> Self {
        let SemanticOperandParts::Authored(authored) = self.parts(authority) else {
            panic!("fixture must be authored")
        };
        let mut authored = authored.clone();
        authored.split_env = split_env;
        Self::authored(authored)
    }

    #[cfg(test)]
    pub(crate) fn with_substitution_runtime(
        &self,
        substitution_runtime: Option<(u64, u64)>,
        authority: SemanticOperandAuthority,
    ) -> Self {
        let SemanticOperandParts::Authored(authored) = self.parts(authority) else {
            panic!("fixture must be authored")
        };
        let mut authored = authored.clone();
        authored.substitution_runtime = substitution_runtime;
        Self::authored(authored)
    }

    /// Token-gated inspection: the forcing boundary decomposes the sealed
    /// operand exactly once per force; without the unforgeable
    /// [`SemanticOperandAuthority`] no consumer can read the internals back
    /// out to route around the boundary.
    pub(crate) fn parts(&self, _authority: SemanticOperandAuthority) -> SemanticOperandParts<'_> {
        match &self.kind {
            SemanticOperandKind::Node {
                store_identity,
                generation,
                node,
                evidence,
            } => SemanticOperandParts::Node {
                store_identity: *store_identity,
                generation: *generation,
                node: *node,
                evidence,
            },
            SemanticOperandKind::Authored(authored) => SemanticOperandParts::Authored(authored),
        }
    }
}

pub(crate) enum SemanticOperandParts<'a> {
    Node {
        store_identity: u64,
        generation: u64,
        node: SemanticNodeId,
        evidence: &'a SemanticOperandEvidence,
    },
    Authored(&'a AuthoredSemanticOperand),
}

impl PartialEq for SemanticOperand {
    fn eq(&self, other: &Self) -> bool {
        match (&self.kind, &other.kind) {
            (
                SemanticOperandKind::Node {
                    store_identity: left_store,
                    generation: left_generation,
                    node: left,
                    ..
                },
                SemanticOperandKind::Node {
                    store_identity: right_store,
                    generation: right_generation,
                    node: right,
                    ..
                },
            ) => left_store == right_store && left_generation == right_generation && left == right,
            (SemanticOperandKind::Authored(left), SemanticOperandKind::Authored(right)) => {
                left.locator == right.locator
                    && left.lexical_scope == right.lexical_scope
                    && left.binder == right.binder
                    && left.substitution == right.substitution
                    && left.split_env == right.split_env
                    && left.substitution_runtime == right.substitution_runtime
            }
            _ => false,
        }
    }
}

impl Eq for SemanticOperand {}

impl Hash for SemanticOperand {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match &self.kind {
            SemanticOperandKind::Node {
                store_identity,
                generation,
                node,
                ..
            } => {
                0u8.hash(state);
                store_identity.hash(state);
                generation.hash(state);
                node.hash(state);
            }
            SemanticOperandKind::Authored(authored) => {
                1u8.hash(state);
                authored.locator.hash(state);
                authored.lexical_scope.hash(state);
                authored.binder.hash(state);
                authored.substitution.hash(state);
                authored.split_env.hash(state);
                authored.substitution_runtime.hash(state);
            }
        }
    }
}

/// Exact value and evidence returned by one force read.
///
/// Fields are PRIVATE and the sole constructor ([`Self::minted`]) is gated
/// on the unforgeable [`SemanticOperandAuthority`]: only the forcing
/// boundary can mint a forced outcome, so no consumer can fabricate one
/// (and its evidence) to feed `mint_node_semantic_operand`.
#[derive(Debug, Clone)]
pub struct ForcedSemanticOperand {
    store_identity: u64,
    generation: u64,
    node: SemanticNodeId,
    evidence: SemanticOperandEvidence,
}

impl ForcedSemanticOperand {
    /// Token-gated construction: the forcing boundary's force read is the
    /// only producer.
    pub(crate) fn minted(
        store_identity: u64,
        generation: u64,
        node: SemanticNodeId,
        evidence: SemanticOperandEvidence,
        _authority: SemanticOperandAuthority,
    ) -> Self {
        Self {
            store_identity,
            generation,
            node,
            evidence,
        }
    }

    #[must_use]
    pub(crate) fn store_identity(&self) -> u64 {
        self.store_identity
    }

    #[must_use]
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    #[must_use]
    pub(crate) fn node(&self) -> SemanticNodeId {
        self.node
    }

    #[must_use]
    pub(crate) fn evidence(&self) -> &SemanticOperandEvidence {
        &self.evidence
    }
}

/// The projection precision one operand force requests.
///
/// Request-owned vocabulary, never an operand axis: it travels on the
/// one-shot [`SemanticOperandForceRequest`] and enters the query-family
/// identity the forcing boundary derives from operand identity — exactly
/// once, alongside the request's unchanged complete
/// [`ProjectionReductionContext`]. The operand carrier itself never
/// stores or duplicates it, and no convenience path may default it: a
/// force that names no precision is the explicit whole-surface case.
///
/// Content-free by construction: store-local node identity only — path
/// segments over known keys plus, for a computed index, the issuing
/// store's own [`SemanticNodeId`]. No content hash, version hash, source
/// text, or span can appear in it.
///
/// Crate-internal: every force/mint entry point that consumes one is
/// `pub(crate)`, so the vocabulary itself is too — an external crate can
/// neither name nor construct a precision.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum SemanticOperandForceProjection {
    /// The explicit whole-surface case — the empty-path `ProjectPath`.
    /// For an authored operand this is the only precision that may
    /// converge on the declaration-source `Instantiate` family; every
    /// selective precision keeps force-owned family identity.
    WholeSurface,
    /// Residual path demand. Intermediate segments run `Navigate`; only
    /// the terminal segment runs at the request context's mode. The
    /// requested key or residual path is thereby known BEFORE the
    /// operand is forced, so no unrelated base surface is requested.
    ///
    /// DERIVED, never caller-supplied: a request spells its residual path
    /// in the request-side [`ForceProjectionSegment`] vocabulary and the
    /// forcing boundary lowers it here. That is what makes an
    /// `IndexKey::Computed` segment safe to hash into family identity —
    /// the boundary has already proven the index node belongs to the
    /// issuing store at the live generation and merged its producer
    /// evidence.
    Path(Arc<[PathSegment]>),
    /// Key-domain demand: the force answers the operand's `keyof`. It
    /// forces only key-producing structure and never member values.
    KeyDomain,
}

impl SemanticOperandForceProjection {
    /// The residual path this demand walks, or `None` when the demand is
    /// not a path projection.
    ///
    /// A [`Self::Path`] is NEVER empty — [`SemanticOperandForceRequest::projecting`]
    /// canonicalises an empty residual path to [`Self::WholeSurface`], so
    /// "walk nothing" has exactly one spelling. Two spellings would alias
    /// one value across two family identities: the projection axis is part
    /// of the authored force family identity, and only the whole-surface
    /// precision converges onto the declaration-source `Instantiate`
    /// family, so an empty `Path` would compute the whole-surface answer
    /// into a second, force-owned entry.
    #[must_use]
    pub fn residual_path(&self) -> Option<&Arc<[PathSegment]>> {
        match self {
            Self::Path(path) => Some(path),
            Self::WholeSurface | Self::KeyDomain => None,
        }
    }

    /// Whether this demand is the explicit whole-surface precision.
    #[must_use]
    pub const fn is_whole_surface(&self) -> bool {
        matches!(self, Self::WholeSurface)
    }
}

/// One segment of a force request's residual path — the REQUEST-side
/// vocabulary, distinct from the derived [`PathSegment`] path that
/// eventually enters family identity.
///
/// A computed index is a SEALED operand, never a bare graph handle. The
/// base operand's store identity, generation, and evidence say nothing
/// about a node minted elsewhere: a handle from a foreign store, or from
/// a superseded generation of this one, is just an integer that the
/// current graph would happily interpret as some unrelated node, and its
/// producer's read facts would never reach the forced candidate. So
/// [`Self::Index`] carries the statically-known [`PropertyKey`] carrier,
/// which cannot spell a computed key at all, and [`Self::ComputedIndex`]
/// is the only spelling for one. The forcing boundary validates the index
/// operand's store/generation and merges its producer evidence BEFORE the
/// base is projected, and only then derives the `IndexKey` the shared
/// path vocabulary carries.
#[derive(Debug, Clone)]
pub(crate) enum ForceProjectionSegment {
    /// A declared member key (`Base.name`).
    Member(PropertyKey),
    /// A statically-known index key (`Base["name"]`, `Base[0]`). The
    /// computed form is structurally unspellable here.
    Index(PropertyKey),
    /// A computed index (`Base[K]`) whose key type is itself a forced
    /// operand — the only way to name a computed index in a force.
    ComputedIndex(ForcedSemanticOperand),
}

/// The projection precision a force REQUEST names, before the boundary
/// validates it.
///
/// Distinct from [`SemanticOperandForceProjection`], which is the derived,
/// hashable precision that enters family identity: this is the caller's
/// spelling, and its [`Self::Path`] carries sealed segments rather than
/// raw graph handles.
#[derive(Debug, Clone)]
pub(crate) enum SemanticOperandForceDemand {
    /// The explicit whole-surface case.
    WholeSurface,
    /// A non-empty residual path in the sealed segment vocabulary.
    Path(Arc<[ForceProjectionSegment]>),
    /// Key-domain (`keyof`) demand.
    KeyDomain,
}

impl SemanticOperandForceDemand {
    /// Whether this demand is the explicit whole-surface precision.
    #[must_use]
    pub const fn is_whole_surface(&self) -> bool {
        matches!(self, Self::WholeSurface)
    }
}

/// One-shot owner of the complete projection-reduction context plus the
/// force's projection demand.
#[derive(Debug)]
pub struct SemanticOperandForceRequest {
    context: ProjectionReductionContext,
    demand: SemanticOperandForceDemand,
}

impl SemanticOperandForceRequest {
    /// A whole-surface force: the operand's entire surface at the request
    /// context's precision. The empty path is the EXPLICIT whole-surface
    /// case, never an accidental side effect of forcing a base.
    pub(crate) fn new(context: ProjectionReductionContext) -> Self {
        Self {
            context,
            demand: SemanticOperandForceDemand::WholeSurface,
        }
    }

    /// A force requesting the residual path spelled by `segments`: the
    /// operand is forced only at that path's precision (intermediate hops
    /// `Navigate`, terminal hop at `context.mode`), so no unrelated base
    /// surface is ever requested to answer one key.
    ///
    /// An empty segment list canonicalises to
    /// [`SemanticOperandForceDemand::WholeSurface`] so the two spellings
    /// of "walk nothing" cannot occupy two family identities for one
    /// value.
    pub(crate) fn projecting(
        context: ProjectionReductionContext,
        segments: Arc<[ForceProjectionSegment]>,
    ) -> Self {
        let demand = if segments.is_empty() {
            SemanticOperandForceDemand::WholeSurface
        } else {
            SemanticOperandForceDemand::Path(segments)
        };
        Self { context, demand }
    }

    /// A force requesting the operand's key domain (`keyof`): only
    /// key-producing structure is forced, never member values.
    pub(crate) fn key_domain(context: ProjectionReductionContext) -> Self {
        Self {
            context,
            demand: SemanticOperandForceDemand::KeyDomain,
        }
    }

    /// The request's complete context and its projection demand. The
    /// request is CONSUMED: the forcing boundary validates the demand,
    /// derives the precision, and combines both with the sealed operand
    /// identity exactly once — nothing stores either axis a second time.
    pub(crate) fn into_parts(self) -> (ProjectionReductionContext, SemanticOperandForceDemand) {
        (self.context, self.demand)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn demand(&self) -> &SemanticOperandForceDemand {
        &self.demand
    }
}

/// Lift a statically-known request segment into the shared path
/// vocabulary. The computed arm is deliberately absent: it needs the
/// forcing boundary's store/generation proof, so it is derived there.
#[must_use]
pub(crate) fn known_segment_to_path_segment(
    segment: &ForceProjectionSegment,
) -> Option<PathSegment> {
    match segment {
        ForceProjectionSegment::Member(key) => Some(PathSegment::Member(key.clone())),
        ForceProjectionSegment::Index(key) => {
            Some(PathSegment::Index(IndexKey::from_known(key.clone())))
        }
        ForceProjectionSegment::ComputedIndex(_) => None,
    }
}

pub(crate) fn authored_anchor(
    locator: &AuthoredBodyLocator,
) -> &verter_type_expr::locators::AuthoredAnchor {
    match locator {
        AuthoredBodyLocator::DeclBody(slot) => &slot.anchor,
        AuthoredBodyLocator::AugmentationBody(body) => &body.anchor,
        AuthoredBodyLocator::JsdocTypedefBody(body) => &body.anchor,
        AuthoredBodyLocator::MacroPayload(payload) => &payload.anchor,
    }
}
