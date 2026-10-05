//! The owned result of dereferencing an authored body locator, and its typed failures.

use std::sync::Arc;
use verter_type_expr::locators::{TypeBodyPathStep, TypeParamBoundPosition, TypeParamVisibility};
use verter_type_expr::{TypeExpr, TypeParam};

/// Why a locator deref could not produce the authored typed IR. Every
/// variant is a typed, fail-closed non-result — a deref NEVER fabricates a
/// body and NEVER falls back to a transient re-parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocatorBodyDerefError {
    /// The locator anchor names a DIFFERENT producing canonical than the memo
    /// serving the deref — a locator must deref through the memo of its OWN
    /// producing canonical (`anchor.canonical_id == memo.key.canonical`). A
    /// unit variant (no payload) keeps this hot error enum cheap; the invariant
    /// is structural, so the identities are not needed on the failure path.
    /// Checked up front for every arm, before any body demand — the typed,
    /// release-present successor to the former branch-local `debug_assert_eq!`.
    CanonicalMismatch,
    /// The macro ordinal exists in the producing canonical, but belongs to a
    /// different exact top-level lexical owner than the locator anchor.
    OwnerMismatch,
    /// The locator anchor names no inventoried declaration. This is a
    /// GENUINE, cacheable resolution result (the symbol truly does not
    /// exist) — DISTINCT from [`Self::LeaseMiss`].
    UnknownSymbol,
    /// The demanded body lowering hit a BROKEN lease pin (`ReturnOnly`): the
    /// lowering ran NOTHING and produced NOTHING. This is a transient no-warm
    /// signal, NOT a cacheable resolution result — the enclosing
    /// `LowerLocator` / `Instantiate` build must refuse warm admission
    /// (`cache_suppress`) so a later demand under a live lease recovers.
    /// Never collapsed into [`Self::UnknownSymbol`].
    LeaseMiss,
    /// The producer-emitted path does not resolve against the authored
    /// body (a stale / out-of-range ordinal, or a shape mismatch).
    PathUnresolved,
    /// A VALUE anchor whose declaration carries no authored type
    /// annotation — there is no authored TYPE body at that position.
    ValueAnnotationAbsent,
    /// A `TypeParamBound` step names a parameter ordinal past the owning
    /// declaration's type-parameter list. Fail-closed, never a fabricated body.
    TypeParamOrdinalOutOfRange { ordinal: u32 },
    /// The referenced type parameter exists but carries no authored body at the
    /// requested bound slot (no constraint for [`TypeParamBoundPosition::Constraint`],
    /// no default for [`TypeParamBoundPosition::Default`]) — analogous to
    /// [`Self::ValueAnnotationAbsent`].
    TypeParamBoundAbsent {
        ordinal: u32,
        position: TypeParamBoundPosition,
    },
    /// A `TypeParamBound` step appears anywhere other than the first path step,
    /// or on a non-type-space anchor. Type parameters live on the declaration
    /// header, not inside the body expression and not on a value / namespace
    /// annotation position, so the step is misplaced by definition. Merged
    /// group-level type parameters are unioned, not per-contributor, so a
    /// contributor-header bound axis does not exist either.
    TypeParamBoundStepMisplaced,
    /// Namespace bodies are not inventoried by the decl-body memo; a
    /// namespace anchor has no memo-backed authored body to deref.
    NamespaceBodyUnrouted,
    /// The macro generic type argument belongs to the analyzer-macro hot
    /// mirror, and no disjoint framework script-fact provider recognized the
    /// locator's ordinal. This remains a typed unroutable result rather than a
    /// fabricated body.
    MacroTypeArgumentHasSoleHotMirrorProducer,
    /// No deref route exists for the whole-object-argument payload position
    /// (no producer mints it): the memo has no demand cell for it, so a
    /// deref for that position fails closed with this typed error rather
    /// than fabricating a body. (The binding-annotation position
    /// (`MacroPayloadPosition::TypeAnnotation`) and the per-field position
    /// (`MacroPayloadPosition::Field`) are HYDRATED — served by the
    /// dedicated `transient_props_annotation_body` /
    /// `transient_macro_field_payload` demand cells, not this unrouted
    /// miss.)
    MacroPayloadPositionUnrouted,
}

/// The derefed authored SHAPE of a locator position: the whole decl body
/// (preserving the distinct merged-contributor carrier) or one
/// path-addressed sub-position.
#[derive(Debug, Clone)]
pub enum DerefedBodyShape {
    /// A single authored body / sub-position expression.
    Single(TypeExpr),
    /// The ordered same-name merged contributors of a whole merged decl
    /// body. Preserved as a DISTINCT carrier — never collapsed to an
    /// intersection (the merged-decl peer-merge reducer needs the
    /// contributor structure).
    Merged(Vec<TypeExpr>),
}

/// Owned typed-IR product of one locator deref: the derefed shape plus the
/// owning declaration's generic parameters and their TS lexical visibility
/// from the derefed position (so the session phase can bind them as
/// `TypeParam` shells in the authored position's own lexical scope under
/// the correct per-position frame). NEVER a `SemanticNodeId` — graph
/// lowering is the session phase's job.
#[derive(Debug, Clone)]
pub struct DerefedAuthoredBody {
    pub shape: DerefedBodyShape,
    pub lexical_root: Option<DerefedLexicalRoot>,
    /// The owning declaration's FULL header type-parameter list, in source
    /// order — never pre-truncated. Which of them the shape may reference
    /// is `visibility`'s to say.
    pub type_parameters: Vec<TypeParam>,
    /// TS lexical visibility of `type_parameters` from the derefed
    /// position: a body position sees every parameter; a constraint bound
    /// sees every sibling (forward refs included); a default bound sees
    /// prior siblings only, with self / later siblings present-as-shadow
    /// but forbidden as references.
    pub visibility: TypeParamVisibility,
}

#[derive(Debug, Clone)]
pub struct DerefedLexicalRoot {
    pub expr: TypeExpr,
    pub path: Arc<[TypeBodyPathStep]>,
}
