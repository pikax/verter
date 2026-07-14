//! Closed semantic fact families — the graph-free, content-free replacement for
//! query-time `TypeExpr` walking.
//!
//! Every family is a CLOSED FINITE enum/struct. There is NO `TypeExpr` /
//! `Box<Self>` open recursive arm / open body arm: unsupported structure is a
//! LOCATOR ([`crate::locators`]) — the single graph-engine-routed escape. Every
//! fact type derives `Eq + Hash + NoTypeExpr + NoStoredSpan` and stores NONE of:
//! `Span` / `MemberSpans` / `FunctionSpans` / `IndexSignatureSpans` / `TypeExpr`
//! / `SemanticNodeId` / `HotTypeRef`. Span information that participates in node
//! identity is carried as a producer-emitted ORIGIN LOCATOR
//! ([`crate::span_origins`]), recovered before identity — never stored as a
//! `Span` field.
//!
//! Adding an arm to any family is a reviewed schema event.

use std::sync::Arc;
use verter_no_storedspan::NoStoredSpan;
use verter_no_typeexpr::NoTypeExpr;

use crate::locators::{
    AuthoredAnchor, AuthoredBodyLocator, AuthoredTypePayloadRef, MacroPayloadLocator,
    SymbolBodyLocator, TypeArgLocator, TypeBodySlot,
};
use crate::span_origins::{
    FunctionParamSpanOrigin, FunctionSpansOrigin, IndexSignatureSpansOrigin, MemberSpansOrigin,
};
use crate::{MemberVisibility, PrimitiveName, TypeExprScope};

// ===========================================================================
// Supporting typed replacements introduced with the fact substrate
// ===========================================================================

/// Typed replacement for the untyped `declaration_origin: Option<Arc<str>>` /
/// `String` member field: the canonical file id the member's declaration lives
/// in, or an explicit synthetic/multi-origin marker.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum DeclarationOrigin {
    /// The member's declaration lives in this canonical file.
    Declared(Arc<str>),
    /// Genuinely synthetic or multi-origin — no single declaring file (the
    /// `None`/empty case in the untyped `declaration_origin` carriers).
    Synthetic,
}

/// The precomputed graph-free target of a `typeof X[.y.z]` value peel — the
/// value-space declaration identity, so the `TypeExpr::TypeOf` walk is replaced
/// by a stored identity. Content-free (canonical + symbol + member path).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct ValueDeclIdentityPart {
    /// Canonical id of the file declaring the value symbol.
    pub canonical_id: Arc<str>,
    /// The value symbol name (`typeof x` → `x`).
    pub symbol: Arc<str>,
    /// Member path for `typeof x.y.z` (empty = the bare value symbol).
    pub member_path: Arc<[String]>,
}

// ===========================================================================
// Surface A facts — authored-shape closed facts (heritage / closedness /
// key-domain), consumed at dispatch time in place of query-time TypeExpr walking.
// ===========================================================================

/// One authored heritage base (an `extends` / `implements` clause). Carries ONLY
/// authored data — the resolved target `(canonical, symbol)` is computed at
/// dispatch time, NEVER stored (a stored resolved identity is a stale-identity
/// R21 hazard).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct HeritageBaseFact {
    /// The base name exactly as written.
    pub name: String,
    /// Authored type arguments as locators (never embedded `TypeExpr`).
    pub type_args: Arc<[TypeArgLocator]>,
    /// The local `name_resolution` map key that routes this base's target at
    /// dispatch time (usually the leading segment of `name`).
    pub name_resolution_ref: String,
    /// Origin locator recovering the base-name span.
    pub base_name_origin: MemberSpansOrigin,
}

/// The role a followed body plays in a closedness decision.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum ClosednessFollowRole {
    /// The subject type whose closedness is being decided.
    Subject,
    /// A key-domain source (e.g. the `Src` of `Pick<Src, K>`).
    KeyDomainSource,
    /// A value / name-type source.
    ValueSource,
}

/// A symbolic substitution binding: a type-parameter name paired with the
/// locator of its authored argument. The live binding environment is RE-MINTED
/// at dispatch time from these — never a stored `SemanticNodeId` / `TypeExpr` /
/// live `KeyDomainBindings`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct SymbolicBinding {
    /// The bound type-parameter name.
    pub param_name: String,
    /// Locator of the authored argument for that parameter.
    pub argument: TypeArgLocator,
}

/// An ordered symbolic binding/substitution environment locator.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct SymbolicBindingLocator {
    /// Ordered bindings (type-param name → authored-argument locator).
    pub bindings: Arc<[SymbolicBinding]>,
}

/// The general closedness escape: follow an authored body under a role and a
/// symbolic binding environment. MUST NOT store a live `KeyDomainBindings`, a
/// borrowed `TypeExpr`, or a `SemanticNodeId` — only the symbolic locator.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct FollowLocatorPayload {
    /// The authored body to follow.
    pub locator: crate::locators::AuthoredBodyLocator,
    /// The role that body plays in the closedness decision.
    pub role: ClosednessFollowRole,
    /// The symbolic binding/substitution environment (re-minted at dispatch).
    pub binding: SymbolicBindingLocator,
}

/// A prep-time KEY-DOMAIN closedness recipe for ONE authored decl-body
/// position, minted at lazy decl-body lowering by the `verter_semantic`
/// producer (`collect_closedness_recipe`) — a pure syntactic extraction.
/// Every arm is BINDING-INDEPENDENT-SOUND: it captures only shapes whose
/// key-domain verdict cannot flip under any live type-parameter binding
/// environment; a binding-dependent leaf carries the NAME the dispatch
/// evaluator resolves against the LIVE environment, and any shape whose
/// verdict needs semantic work (head resolution, per-argument key-domain
/// judgement, conditional branch selection) escapes via
/// [`ClosednessRecipe::LowerAndClassify`] to the ONE node-route walker —
/// never a stored verdict, never an embedded `TypeExpr`.
///
/// Parentheses are normalized away at production time (they carry no semantic
/// content, and the locator navigation unwraps them transparently), so there
/// is NO `Parenthesized` arm; the ONLY composition arm is
/// [`ClosednessRecipe::AllArms`].
///
/// The marker witnesses are DERIVED with the opt-in recursive-self escape
/// (`#[no_typeexpr(recursive_self)]` / `#[no_storedspan(recursive_self)]`): for
/// the fixed-point `AllArms(Arc<[ClosednessRecipe]>)` arm the derive
/// emits a compiler-resolved `RecursiveSelfArc<Self>` PROOF-BOUND instead of the
/// plain witness bound (which would otherwise ask the trait solver to prove
/// `Arc<[Self]>: Marker` while proving `Self: Marker`, an overflow — E0275),
/// while still emitting the per-field witness bound on EVERY non-recursive arm
/// payload. Only the genuine `std::sync::Arc<[ClosednessRecipe]>` satisfies the
/// proof-bound, so a bare/shadowed/custom `Arc` cannot masquerade as the approved
/// self-container; and the future-arm gap stays closed: a NEW non-recursive arm
/// carrying a `TypeExpr` / `Span` would fail the derive (a compile-fail fixture
/// proves this).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
#[no_typeexpr(recursive_self)]
#[no_storedspan(recursive_self)]
pub enum ClosednessRecipe {
    /// A literal / primitive leaf — a finite scalar surface, closed in every
    /// binding environment.
    ClosedLeaf,
    /// An object whose index-signature KEYS are all syntactically closed
    /// scalars (or which has no index signatures): its NAMED member set fixes
    /// the key domain regardless of member values, in every binding
    /// environment. An object with a non-scalar index-signature key escapes
    /// via [`Self::LowerAndClassify`] instead (the key needs the walker).
    ObjectClosed,
    /// A function / constructor-type body: not an enumerable key surface —
    /// decided-open at the key domain in every binding environment.
    OpenLeaf,
    /// A union / intersection composition: closed iff every arm's recipe is
    /// closed. The ONLY composition arm (parentheses are normalized away). On
    /// this arm only, the recursive-self escape REPLACES the plain marker
    /// witness bound with a compiler-resolved `RecursiveSelfArc<Self>`
    /// proof-bound (only the genuine `std::sync::Arc<[Self]>` satisfies it).
    AllArms(Arc<[ClosednessRecipe]>),
    /// A first-class type-parameter reference — the dispatch evaluator
    /// resolves `name` against the LIVE binding environment: bound-closed ⇒
    /// closed leaf, bound-open or FREE ⇒ open.
    ParamRef {
        /// The referenced type-parameter name.
        name: String,
    },
    /// A bare zero-argument reference — the transparent alias hop. The
    /// dispatch evaluator checks the LIVE binding environment first (a bound
    /// parameter spelled as a bare ref), then resolves `name` through the
    /// prepared decl's own `name_resolution` and recurses on the target
    /// decl's fact (budget + in-flight cycle-guarded).
    FollowRefByName {
        /// The referenced name (also the `name_resolution` routing key).
        name: String,
    },
    /// An indexed access `Object[Index]` — judged OPERAND-WISE because the
    /// two operands carry DIFFERENT position policies: the OBJECT operand is
    /// VALUE-SENSITIVE (`Wrap<T>['a']` IS the member value `a`, so an
    /// open-argument object opens the access) while the INDEX stays a
    /// key/keyspace question. A whole-position escape would let the shared
    /// lowerer execute a literal access over a literal object and judge only
    /// the RESULT at the surrounding key-domain position — losing the
    /// value-sensitive operand rule. Each operand derefs + lowers ALONE and
    /// classifies at its pinned position.
    ValueProjection {
        /// Locator of the authored OBJECT operand (value-sensitive).
        object: TypeBodySlot,
        /// Locator of the authored INDEX operand (key-domain).
        index: TypeBodySlot,
    },
    /// The general escape for any shape whose key-domain verdict needs
    /// semantic work (a generic/builtin instantiation, a conditional, a
    /// mapped/keyof/template/tuple/array operator, an import-type
    /// carrier, …): the dispatch evaluator derefs + shallow-lowers the
    /// authored position under the live binding environment through the ONE
    /// shared lowerer and classifies the NODE with the node-route walker.
    LowerAndClassify {
        /// Content-free locator of the authored body position.
        slot: TypeBodySlot,
    },
    /// A shape the key-domain question cannot classify from syntax OR the
    /// node route (`typeof` value queries, recursive-ref placeholders,
    /// synthetic carriers, unlowerable fragments): UNAVAILABLE — never a
    /// false open/closed claim.
    Unsupported,
}

/// The producer-minted per-declaration KEY-DOMAIN closedness fact — the
/// content-free replacement for query-time `TypeExpr` closedness walking over
/// the lease-re-borrowed authored bodies. Minted ONCE at lazy decl-body
/// lowering from the SAME transient contributor bodies the fingerprint
/// observes; carried on the lowered record and COPIED onto the prepared decl.
/// `None`-carried (seeded / enum groups) reads as UNAVAILABLE, never as a
/// verdict.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct KeyDomainClosednessFact {
    /// Whether EVERY contributor body is a closed-object SHAPE (an `Object`,
    /// an intersection of closed-object shapes, or a parenthesized chain of
    /// those) — the nominal-interface carve-out verdict the publication
    /// terminals consult (`userland_instantiation_body_is_closed_object`).
    /// Pure syntax: index-signature keys and member values are NOT consulted
    /// (a nominal object surface stays a carrier regardless of its values).
    pub closed_object_shape: bool,
    /// Per-contributor closedness recipes, in source/binder order (a
    /// JSDoc-`@typedef` payload body appends last — mirroring the transient
    /// body order the previous query-time walk consumed).
    pub body_recipes: Arc<[ClosednessRecipe]>,
}

/// The owned prep fact for a key domain — recipe-only arms. The live borrowed
/// `KeyDomainBinding` arms (`ClosedExpr` / `ClosedNode`) are NEVER stored; they
/// are re-minted during dispatch evaluation.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum KeyDomainFact {
    /// The key domain is open.
    Open,
    /// The key domain is closed but abstract (no concrete enumeration).
    ClosedAbstract,
    /// Follow the referenced slot to decide the key domain.
    FollowSlot(SymbolBodyLocator),
    /// The general escape — follow an authored body under a symbolic binding.
    FollowLocator(FollowLocatorPayload),
}

/// A content-free alias reference inside a [`KeySourceFact`]: one zero-argument
/// bare `Ref` arm of a deferred key-source alias body, addressed by its
/// authored anchor. NOT resolved at produce time — the session engine follows
/// the anchor by canonical declaration id at demand time, failing closed on any
/// unresolved hop.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct KeySourceRefFact {
    /// The referenced alias declaration (producing canonical + symbol + space).
    pub anchor: AuthoredAnchor,
}

/// The FLAT normalized deferred key-source fact of ONE declaration — the
/// content-free replacement for handing raw alias bodies to the route-closure
/// core. Produced LOCALLY and NON-TRANSITIVELY from the declaration's lowered
/// contributor bodies: string literals flatten through top-level unions and
/// parentheses into `literals`; zero-argument bare refs become UNRESOLVED
/// [`KeySourceRefFact`] alias arms; any other single-body shape — and any
/// multi-contributor (merged) surface — enumerates no finite keys. Alias
/// following (cross-decl, cycles, availability) is the session engine's job at
/// demand time, never the producer's.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum KeySourceFact {
    /// The declaration enumerates no finite literal key set (an object /
    /// merged / operator / generic-instantiation body): a zero-key
    /// contribution, distinct from an UNAVAILABLE hand-off.
    NoFiniteKeys,
    /// A finite literal-union surface: the flattened literal arms plus the
    /// unresolved alias-ref arms (either may be empty; non-conforming union
    /// arms contribute nothing, exactly like the legacy enumeration).
    LiteralAliasUnion {
        /// Flattened string-literal arms, in source order (unsorted — the
        /// engine sorts/dedups only a COMPLETED enumeration).
        literals: Arc<[String]>,
        /// Unresolved zero-argument alias-ref arms, in source order.
        aliases: Arc<[KeySourceRefFact]>,
    },
}

// ===========================================================================
// Surface B facts — graph-free frontier / shallow / eval-env locators + finite
// facts (the graph-free boundary that precedes the graph, never a HotTypeRef).
// ===========================================================================

/// A CLOSED frontier body — ONLY unresolved-symbolic arms plus the locator
/// escape. Deliberately NO object-members arm and NO general body arm.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum NarrowFrontierBody {
    /// A re-export route (specifier + exported name), no resolved body.
    ExportRoute {
        specifier: String,
        exported_name: String,
    },
    /// An unresolved external ref (name only).
    UnresolvedExternalRef { name: String },
    /// An unbound type-parameter shell.
    TypeParamShell { name: String },
    /// The escape: a resolvable body addressed by locator.
    Resolvable(SymbolBodyLocator),
}

/// The object-member-names route: a closed enumeration, or the open/undecidable
/// carrier-stop class (`OpenKeyDomain`).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum MemberNamesRoute {
    /// A closed enumeration of object member names.
    Closed(Arc<[String]>),
    /// Open / undecidable key domain — the L1 carrier-stop class.
    OpenKeyDomain,
}

/// A normalized Pick/Omit member-key SET: the inner slice is PRIVATE and kept
/// sorted + deduped at construction AND at serde decode, so the DERIVED `Eq`
/// and `Hash` operate on the same normalized representation and therefore
/// AGREE (both order-independent). This fixes the latent cache-identity bug
/// the pre-collapse pair carried (order-sensitive derived `Eq` combined with
/// sort-before-hash `Hash` — two values could hash equal while comparing
/// unequal). Membership semantics only — Pick/Omit key ORDER is not semantic.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, NoTypeExpr, NoStoredSpan)]
pub struct RouteKeySet(Arc<[String]>);

impl RouteKeySet {
    /// Build a normalized key set (sorted + deduped).
    pub fn new<I, S>(keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut keys: Vec<String> = keys.into_iter().map(Into::into).collect();
        keys.sort();
        keys.dedup();
        Self(keys.into())
    }

    /// The normalized (sorted, deduped) keys.
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }

    /// Iterate the normalized keys.
    pub fn iter(&self) -> std::slice::Iter<'_, String> {
        self.0.iter()
    }

    /// Number of distinct keys.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Membership test (binary search over the normalized inner).
    pub fn contains(&self, key: &str) -> bool {
        self.0
            .binary_search_by(|probe| probe.as_str().cmp(key))
            .is_ok()
    }
}

impl<'a> IntoIterator for &'a RouteKeySet {
    type Item = &'a String;
    type IntoIter = std::slice::Iter<'a, String>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Decode re-normalizes (sorted + deduped): a hand-crafted or legacy-ordered
/// payload can never smuggle an unnormalized inner past the smart constructor.
impl<'de> serde::Deserialize<'de> for RouteKeySet {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let keys = Vec::<String>::deserialize(deserializer)?;
        Ok(Self::new(keys))
    }
}

/// How much of an exported symbol's dependency graph a route needs — the ONE
/// canonical route-demand type, shared by the session resolver
/// (`resolver_core` re-exports it) and the fact substrate. `Pick`/`Omit`
/// carry the normalized [`RouteKeySet`] (order-independent `Eq` + `Hash`);
/// `MemberPath` stays an ordered sequence (`Type['a']['b']` ≠ `Type['b']['a']`
/// — order-sensitive `Eq` + `Hash`). All derives are plain: no hand-written
/// `Hash` remains, so `Eq`/`Hash` cannot drift apart again.
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum RouteDemand {
    /// Full export — all dependencies.
    #[default]
    Whole,
    /// Indexed member path: `Type['a']['b']` (each element is one segment; never
    /// collapsed to a shorter prefix).
    MemberPath(Arc<[String]>),
    /// `Pick<Type, 'a' | 'b'>` subset (normalized key set).
    Pick(RouteKeySet),
    /// `Omit<Type, 'a' | 'b'>` subset (normalized key set).
    Omit(RouteKeySet),
}

impl RouteDemand {
    /// Build a `Pick` demand from any key iterator (normalized).
    pub fn pick<I, S>(keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::Pick(RouteKeySet::new(keys))
    }

    /// Build an `Omit` demand from any key iterator (normalized).
    pub fn omit<I, S>(keys: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::Omit(RouteKeySet::new(keys))
    }

    /// Build a `MemberPath` demand from ordered segments (order preserved).
    pub fn member_path<I, S>(segments: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::MemberPath(
            segments
                .into_iter()
                .map(Into::into)
                .collect::<Vec<String>>()
                .into(),
        )
    }

    /// The normalized subset keys of a `Pick`/`Omit` demand.
    pub fn keys(&self) -> Option<&RouteKeySet> {
        match self {
            Self::Pick(keys) | Self::Omit(keys) => Some(keys),
            Self::Whole | Self::MemberPath(_) => None,
        }
    }

    /// The ordered segments of a `MemberPath` demand.
    pub fn segments(&self) -> Option<&[String]> {
        match self {
            Self::MemberPath(segments) => Some(segments),
            Self::Whole | Self::Pick(_) | Self::Omit(_) => None,
        }
    }
}

/// Merge two route demands conservatively — the narrowest demand that
/// satisfies both requests (used when multiple consumers request the same
/// symbol with different routes). Pure over the canonical [`RouteDemand`];
/// the session resolver re-exports it.
pub fn merge_route_demands(a: &RouteDemand, b: &RouteDemand) -> RouteDemand {
    if a == b {
        return a.clone();
    }
    match (a, b) {
        (RouteDemand::Whole, _) | (_, RouteDemand::Whole) => RouteDemand::Whole,
        (RouteDemand::MemberPath(pa), RouteDemand::MemberPath(pb)) => {
            let common_prefix = pa
                .iter()
                .zip(pb.iter())
                .take_while(|(left, right)| left == right)
                .map(|(segment, _)| segment.clone())
                .collect::<Vec<_>>();
            if !common_prefix.is_empty() {
                RouteDemand::member_path(common_prefix)
            } else {
                let mut members = Vec::new();
                if let Some(first) = pa.first() {
                    members.push(first.clone());
                }
                if let Some(first) = pb.first() {
                    members.push(first.clone());
                }
                if members.is_empty() {
                    RouteDemand::Whole
                } else {
                    RouteDemand::pick(members)
                }
            }
        }
        (RouteDemand::MemberPath(p), RouteDemand::Pick(ps))
        | (RouteDemand::Pick(ps), RouteDemand::MemberPath(p)) => {
            let mut merged: Vec<String> = ps.as_slice().to_vec();
            if let Some(first) = p.first() {
                merged.push(first.clone());
            }
            if merged.is_empty() {
                RouteDemand::Whole
            } else {
                RouteDemand::pick(merged)
            }
        }
        (RouteDemand::Pick(a), RouteDemand::Pick(b)) => {
            let mut merged: Vec<String> = a.as_slice().to_vec();
            merged.extend(b.iter().cloned());
            RouteDemand::pick(merged)
        }
        (RouteDemand::Omit(a_omit), RouteDemand::MemberPath(p)) => {
            // Omit + MemberPath: if the member is not omitted, it's still valid
            if p.first().is_some_and(|first| !a_omit.contains(first)) {
                RouteDemand::Omit(a_omit.clone())
            } else {
                RouteDemand::Whole
            }
        }
        (RouteDemand::MemberPath(p), RouteDemand::Omit(b_omit)) => {
            if p.first().is_some_and(|first| !b_omit.contains(first)) {
                RouteDemand::Omit(b_omit.clone())
            } else {
                RouteDemand::Whole
            }
        }
        // Omit + Pick, Omit + Omit: conservatively widen to Whole
        _ => RouteDemand::Whole,
    }
}

/// A whole external route reference — the lower-neutral, content-free mirror of a
/// session `ExternalSymbolRef`, 1:1 by field. Carries the local import name, the
/// authored specifier + imported name, the optionally-resolved canonical id, and
/// the remaining route demand, so `MemberPath` / `Pick` / `Omit`, the canonical,
/// and route transitivity survive the narrowing (a bare name string would drop
/// them). The session boundary converts this to/from `ExternalSymbolRef` 1:1
/// (deterministic order, dedupe by `(source_specifier, imported_name)`, route
/// merge by `merge_route_demands`).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct ExternalRouteRefFact {
    /// The local import name in the owning file.
    pub local_name: String,
    /// The authored import specifier (e.g. `./types`, `reka-ui`).
    pub source_specifier: String,
    /// The original exported name in the source module.
    pub imported_name: String,
    /// The resolved canonical file id of the target, when the specifier resolved
    /// (`None` for an unresolved / not-yet-resolved specifier).
    pub canonical_id: Option<Arc<str>>,
    /// The remaining route demand on the imported symbol.
    pub route: RouteDemand,
}

/// One dependency reference in a route closure — either a same-file local name or
/// an external route ref. The typed replacement for a bare dependency name
/// string: a local dep stays a name plus its route demand, an external dep keeps
/// its full route ref, so member / Pick / Omit closures never reclassify a name
/// or widen an imported route.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum RouteDependencyRefFact {
    /// A same-file local dependency, by name, with the route demanded of it
    /// (`Whole` for direct member / whole-route local deps; narrower demands
    /// for routed local follows — member-path / RouteDb uniformity).
    Local {
        /// The local symbol name depended on.
        name: String,
        /// The route demanded of the local symbol.
        route: RouteDemand,
    },
    /// An external (imported) dependency, carrying its full route ref.
    External(ExternalRouteRefFact),
}

/// One per-member dependency edge (a NAME/REF enumeration, not a type-shape
/// evaluation).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct MemberDependencyEdge {
    /// The member whose dependencies these are.
    pub member: String,
    /// The typed route refs this member depends on — each a local name or an
    /// external route ref carrying its route demand (never a bare name string,
    /// which would drop Pick / Omit / canonical / route transitivity).
    pub depends_on: Arc<[RouteDependencyRefFact]>,
}

/// The whole-route walk context at a produced edge's site — the produce-time
/// walk starts at `Root`; object property / index-signature values walk under
/// `LeafProperty`; callable parameter positions (params + function type-param
/// bounds) walk under `CallableParam`. `Root` and `CallableParam` are
/// behaviorally identical EMITTING contexts (every legacy gate tests them
/// jointly); `LeafProperty` is the suppressing context (import emits are
/// gated off; object/function tops stop). The per-edge context is
/// byte-parity-load-bearing: the downstream transitive closure composes it
/// with the follow context (a `LeafProperty` follow processes ONLY edges
/// whose stored context is `Root` — the fully-transparent sites — and
/// re-applies the import-emit gate there).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum WholeRouteContextFact {
    /// The emitting root context (also recorded for sites reachable only
    /// through an emitting-only carrier such as a `Partial`-family type
    /// argument — those normalize to [`CallableParam`](Self::CallableParam)).
    Root,
    /// A callable parameter position (params, function type-param bounds) —
    /// emitting, but guarded (unreachable under a `LeafProperty` follow).
    CallableParam,
    /// An object property / index-signature value position — suppressing.
    LeafProperty,
}

/// The utility family of a deferred-key edge.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum DeferredKeyUtilityKind {
    /// `Pick<Base, KeySource>`.
    Pick,
    /// `Omit<Base, KeySource>`.
    Omit,
    /// `Base[...literal path...][KeySource]` — an indexed access whose
    /// OUTERMOST index is the deferred key source (inner indices all literal,
    /// carried as `base_path`).
    IndexedAccess,
}

/// The deferred cross-decl key-source edge: a `Pick` / `Omit` / indexed
/// access whose key argument is a BARE local alias (`Pick<Imported, LocalKeys>`)
/// that produce-time literal extraction cannot enumerate without a cross-decl
/// follow. The producer records the RECIPE ONLY (the [`KeyDomainFact::FollowSlot`]
/// locator of the local key-source alias); enumeration runs DOWNSTREAM through
/// the existing key-domain machinery (re-minted at dispatch — never the
/// producer), and FAILS CLOSED (the edge contributes nothing) when the key
/// source cannot be followed or does not enumerate to literal keys.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct DeferredKeyUtilityEdge {
    /// Which utility family defers its keys.
    pub kind: DeferredKeyUtilityKind,
    /// The utility's source/base argument, when it is a bare (paren-transparent,
    /// no-type-args) local or import ref — `None` for a complex base, which
    /// contributes nothing even after key resolution (matching the legacy
    /// routed-expr fall-through). A `Local` base carries `RouteDemand::Whole`
    /// and an `External` base's ref carries `RouteDemand::Whole` as produce-time
    /// PLACEHOLDERS — the downstream closure substitutes the resolved
    /// Pick/Omit/MemberPath route.
    pub base: Option<RouteDependencyRefFact>,
    /// The literal inner index path for [`DeferredKeyUtilityKind::IndexedAccess`]
    /// (`Imported['a'][K]` carries `["a"]`); empty for `Pick`/`Omit`.
    pub base_path: Arc<[String]>,
    /// The deferred key-source recipe (the bare local alias, as a
    /// [`KeyDomainFact::FollowSlot`] symbol locator).
    pub key_source: KeyDomainFact,
    /// For `Pick`/`Omit` only: the userland LOCAL type named `Pick`/`Omit` to
    /// whole-follow when the keys resolve EMPTY (the legacy `utility → None →
    /// has_type_symbol(name)` fall-through). `None` when no such local decl
    /// exists (the common case — empty keys then contribute nothing).
    pub empty_keys_fallback: Option<String>,
    /// The site context (same composition rules as every whole-route edge).
    pub context: WholeRouteContextFact,
}

/// One DIRECT whole-route edge of a declaration's own body walk — a deferred
/// local follow, a direct external emit, or a deferred-key utility. The
/// producer walks the decl's OWN body once from `Root` (graph-free — no
/// cross-decl follow); the downstream closure reproduces the legacy transitive
/// walk over these direct edges. Edge ORDER is the legacy walk's depth-first
/// emission order — the downstream closure processes edges in stored order so
/// external accumulation order is byte-identical.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum WholeRouteEdgeFact {
    /// A same-file local reference to follow downstream: `route` is `Whole`
    /// for a plain ref follow, or the utility/indexed route for a routed local
    /// follow (`Pick`/`Omit` → sub-closure; `MemberPath` → seed walk).
    Local {
        /// The referenced local symbol name (may be a header miss — the
        /// downstream `has_type_symbol` gate no-ops it, charging no budget).
        name: String,
        /// The demanded route on the local symbol.
        route: RouteDemand,
        /// The site context (composition-load-bearing).
        context: WholeRouteContextFact,
    },
    /// A direct external emit. `route == Whole` ⟺ a context-gated Ref/TypeOf
    /// import emit; `route != Whole` ⟺ an ungated utility/indexed emit. The
    /// per-edge `context` distinguishes a TRANSPARENT site (context `Root` —
    /// survives a `LeafProperty` follow when ungated) from a GUARDED site
    /// (context `CallableParam`/`LeafProperty` — dropped under a
    /// `LeafProperty` follow): two decls storing the same 5-field ref can
    /// require opposite Leaf-follow behavior (`type B = Pick<Q,'a'>` keeps Q
    /// under a leaf follow; `type B = { y: Pick<Q,'a'> }` stops at the object
    /// top), so the context is stored per edge.
    External {
        /// The external route ref (5-field, dedup/merge key downstream).
        external_ref: ExternalRouteRefFact,
        /// The site context (composition-load-bearing).
        context: WholeRouteContextFact,
    },
    /// A deferred cross-decl key-source utility edge.
    DeferredKeyUtility(DeferredKeyUtilityEdge),
}

/// The target of one member-path seed edge — terminal deps at an exact
/// property path, or a bare-ref forward boundary that appends the remaining
/// query tail.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum MemberPathSeedTarget {
    /// `path` is a terminal property (non-ref-carrier position): its direct
    /// refs, each demanded `RouteDemand::Whole`. Matches a query path ONLY
    /// when `query_path == path` EXACTLY.
    TerminalDeps(Arc<[RouteDependencyRefFact]>),
    /// `path` (possibly empty) is a strict prefix; the property/root type is a
    /// BARE REF CARRIER (local/import `Ref`, no type args, parens-transparent).
    /// The downstream walk forwards the remaining tail
    /// `query_path[path.len()..]` as `MemberPath(tail)`: import ⇒ emit the
    /// external with that route; local ⇒ recurse that decl's seed edges with
    /// the tail (cycle-guarded). Any union/conditional/mapped/indexed/generic/
    /// complex terminal at a descended position produces NO edge — the
    /// fail-closed MISS (never over-produce).
    ForwardBoundary(RouteDependencyRefFact),
}

/// One member-path seed edge: a property path within the decl's own direct
/// object structure plus its target. The producer enumerates the decl's OWN
/// object structure by prefix (direct properties through
/// object/intersection/parenthesized descent — no cross-decl follow); the
/// downstream walk does the cross-decl forwarding.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct MemberPathSeedEdge {
    /// The property path within the owning decl (empty = the decl's own body
    /// root, for the whole-body-is-a-bare-ref forward case).
    pub path: Arc<[String]>,
    /// What the path resolves to.
    pub depends_on: MemberPathSeedTarget,
}

/// The shallow route closures narrowed to closed NAME/REF facts — the per-decl
/// DIRECT route facts the producer emits at lazy decl-body lowering (graph-free:
/// the producer walks its own transient contributor bodies once; same-file
/// TRANSITIVE closure and cycle/budget live downstream in the session resolver,
/// reading sibling decls' stored facts).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct ShallowRouteFacts {
    /// Object-member-names route: `Closed(names)` for a direct-object body
    /// (enumeration order; empty = not a direct object with properties — the
    /// downstream Omit arm falls back to the plain local closure), or the
    /// genuine open/undecidable carrier-stop marker.
    pub member_names: MemberNamesRoute,
    /// Member-path seed edges (own-object prefix enumeration; terminal deps +
    /// bare-ref forward boundaries).
    pub member_path_seed_edges: Arc<[MemberPathSeedEdge]>,
    /// Per-member dependency edges.
    pub member_dependency_edges: Arc<[MemberDependencyEdge]>,
    /// DIRECT whole-route edges of the decl's own body walk, in legacy
    /// depth-first emission order (local follows deferred downstream — never a
    /// baked transitive closure, never a baked status).
    pub whole_route_edges: Arc<[WholeRouteEdgeFact]>,
}

impl ShallowRouteFacts {
    /// The canonical EMPTY facts (a body-less/ref-less declaration — e.g. an
    /// enum's scalar-union projection or a synthesized ambient decl).
    pub fn empty() -> Self {
        Self {
            member_names: MemberNamesRoute::Closed(Arc::from(Vec::new().into_boxed_slice())),
            member_path_seed_edges: Arc::from(Vec::new().into_boxed_slice()),
            member_dependency_edges: Arc::from(Vec::new().into_boxed_slice()),
            whole_route_edges: Arc::from(Vec::new().into_boxed_slice()),
        }
    }
}

/// One direct member header fact — the narrowed, content-free replacement for a
/// merged-body walk when deriving a declaration's `DeclHeaderIndex` member
/// headers. Carries a member's NAME plus the header-level flags
/// `decl_headers.rs::from_eval_env` reconstructs into a `MemberHeader`
/// (`name` / `kind` / `optional` / `readonly`) plus its `visibility` (tracked on
/// the shared member surface). A narrowed
/// `TypeDeclInfo.direct_member_headers: Arc<[MemberHeaderFact]>` reads these
/// facts instead of walking `merged_body().merged_member_names()`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct MemberHeaderFact {
    /// The member name.
    pub name: String,
    /// Whether the member is a method (`true`) vs a property (`false`) — the
    /// lower-neutral analogue of the shallow `MemberHeaderKind::{Method,
    /// Property}`.
    pub is_method: bool,
    /// Whether the member is optional (`?`).
    pub optional: bool,
    /// Whether the member is readonly.
    pub readonly: bool,
    /// Member visibility (publication-filtered at the boundary; part of identity).
    pub visibility: MemberVisibility,
}

/// The narrowed enum-member-NAME inventory fact — the full statically-named
/// SUPERSET (`merged_enum_member_names`: every named member including
/// unfoldable-value and computed-name members) that the value rail
/// ([`EnumMemberFact`] / `merged_enum_members`) filters. An `enum` value
/// declaration carries this so `decl_headers.rs::from_eval_env` reads a fact for
/// its `enum_headers` entry instead of walking the merged enum body.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct EnumMemberNamesFact {
    /// The ordered enum member names (the full statically-named superset).
    pub names: Arc<[String]>,
}

/// Classification of a value annotation body.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum ValueAnnotationClass {
    /// A `typeof X` peel target (paired with `typeof_alias_target`).
    TypeOfAlias,
    /// A direct annotation source (reached via the `annotation` field) — an
    /// authored TS annotation body OR, for an inferred / default-expression type
    /// with no authored `TSType` node, a closed / synthesized fact.
    Direct,
    /// No annotation.
    Absent,
}

/// The `PreparedValueDecl.type_annotation` narrowing: a precomputed
/// `typeof_alias_target` (replacing the `TypeExpr::TypeOf` peel) plus a
/// classification and, when present, the annotation source.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct ValueTypeAnnotationFact {
    /// The precomputed graph-free `typeof x[.y]` target, when the annotation is a
    /// value peel.
    pub typeof_alias_target: Option<ValueDeclIdentityPart>,
    /// The annotation classification.
    pub classification: ValueAnnotationClass,
    /// The annotation source, when present. Widened from a bare authored body
    /// locator to the four-source [`SemanticTypeSource`] so an INFERRED /
    /// default-expression annotation (which has no authored `TSType` node to
    /// address with a [`TypeBodySlot`]) is carried as a
    /// [`SemanticTypeSource::Closed`] / [`SemanticTypeSource::Synthesized`] closed
    /// fact rather than a fabricated authored locator; an authored TS annotation
    /// is [`SemanticTypeSource::Authored`]. Absent for
    /// [`ValueAnnotationClass::Absent`].
    pub annotation: Option<SemanticTypeSource>,
}

/// One narrowed type parameter: its name, ordinal, and constraint/default
/// locators (never embedded `TypeExpr`).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct NarrowTypeParam {
    /// The type-parameter name.
    pub name: String,
    /// Its declaration ordinal within the owning type-parameter list.
    pub ordinal: u32,
    /// Constraint body locator (`T extends C`), if any.
    pub constraint: Option<TypeBodySlot>,
    /// Default body locator (`T = D`), if any.
    pub default: Option<TypeBodySlot>,
}

/// A whole type-parameter declaration list.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct TypeParamDeclFact {
    /// The ordered type parameters.
    pub params: Arc<[NarrowTypeParam]>,
}

// ===========================================================================
// Surface C facts — lower-crate Prepared* / Analyzed* / Projected* facts +
// locators, held in place inside verter_semantic (never a HotTypeRef).
// ===========================================================================

/// Structural classification of a prepared type-decl body.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum TypeBodyClass {
    /// `type X = ...`.
    Alias,
    /// `interface X { ... }`.
    Interface,
    /// `class X { ... }`.
    Class,
    /// Multiple same-name `interface X` declarations (merged).
    MergedInterface,
}

/// The `PreparedTypeDecl.body: TypeExpr` narrowing — a classification plus the
/// body slot locator and the ordered merged-contributor slots.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct PreparedTypeBodyFacts {
    /// The body classification.
    pub classification: TypeBodyClass,
    /// The body slot locator.
    pub body_slot: TypeBodySlot,
    /// Ordered merged-declaration contributor slots (empty for non-merged).
    pub merged_contributor_slots: Arc<[TypeBodySlot]>,
}

/// A narrowed function parameter fact. `FunctionParam.span` participates in the
/// hand-written identity, so a `FunctionParamSpanOrigin` recovers it.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct FunctionParamFact {
    /// The parameter name, if any.
    pub name: Option<String>,
    /// Whether the parameter is optional.
    pub optional: bool,
    /// Whether the parameter is a rest parameter.
    pub rest: bool,
    /// Whether an explicit TS annotation was authored (identity-relevant fact).
    pub has_ts_annotation: bool,
    /// The parameter type body locator. `Some` ONLY for an authored positional
    /// TS annotation (a `params.items[ordinal]` parameter with an authored
    /// `TSType` — the one position [`TypeBodyPathStep::FunctionParam`] derefs).
    /// An unannotated parameter and a REST parameter store `None` — the typed
    /// miss; both are recovered whole-signature on demand, never through a
    /// fabricated slot.
    ///
    /// [`TypeBodyPathStep::FunctionParam`]: crate::locators::TypeBodyPathStep::FunctionParam
    pub ty: Option<TypeBodySlot>,
    /// Origin locator recovering `FunctionParam.span`.
    pub span_origin: FunctionParamSpanOrigin,
}

/// A narrowed function signature (an overload-group member). `FunctionExpr`
/// carries `FunctionSpans` in identity, recovered via `spans_origin`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct FunctionSignatureFact {
    /// The function's own type parameters.
    pub type_parameters: Arc<[NarrowTypeParam]>,
    /// Ordered parameter facts.
    pub parameters: Arc<[FunctionParamFact]>,
    /// The return type body locator (absent = inferred / void).
    pub return_ty: Option<TypeBodySlot>,
    /// Overload-visibility fact: hide the trailing implementation signature.
    pub has_implementation_body: bool,
    /// Origin locator recovering `FunctionSpans`.
    pub spans_origin: FunctionSpansOrigin,
}

/// A narrowed object property member. Carries the identity-participating
/// `visibility` + `optional` + `readonly`; the member span is recovered via
/// `span_origin`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct ObjectPropertyFact {
    /// The member name.
    pub name: String,
    /// Whether the member is optional.
    pub optional: bool,
    /// Whether the member is readonly.
    pub readonly: bool,
    /// Member visibility (identity-participating, publication-filtered).
    pub visibility: MemberVisibility,
    /// The member value type body locator.
    pub ty: TypeBodySlot,
    /// Origin locator recovering the member's `MemberSpans`.
    pub span_origin: MemberSpansOrigin,
}

/// A narrowed object method member.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct ObjectMethodFact {
    /// The member name.
    pub name: String,
    /// Whether the method is optional.
    pub optional: bool,
    /// Member visibility (identity-participating, publication-filtered).
    pub visibility: MemberVisibility,
    /// The method's function signature.
    pub function: FunctionSignatureFact,
    /// Origin locator recovering the member's `MemberSpans`.
    pub span_origin: MemberSpansOrigin,
}

/// The declared SHAPE of an index-signature key (so `[k: string] ≠ [k: number]`).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum KeyTypeShape {
    /// `[k: string]`.
    String,
    /// `[k: number]`.
    Number,
    /// `[k: symbol]`.
    Symbol,
    /// A non-primitive / complex key type addressed by locator (fact-or-locator).
    Other(TypeBodySlot),
}

/// A narrowed index signature. `IndexSignature` carries `IndexSignatureSpans` in
/// identity, recovered via `span_origin`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct IndexSignatureFact {
    /// The index key parameter name (`k`).
    pub key_name: String,
    /// The declared key SHAPE.
    pub key_type: KeyTypeShape,
    /// The value type body locator.
    pub value_type: TypeBodySlot,
    /// Whether the index signature is readonly.
    pub readonly: bool,
    /// Origin locator recovering the `IndexSignatureSpans`.
    pub span_origin: IndexSignatureSpansOrigin,
}

/// One narrowed object member over all five `ObjectMember` variants.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum ObjectMemberFact {
    /// A property member.
    Property(ObjectPropertyFact),
    /// A method member.
    Method(ObjectMethodFact),
    /// A call signature.
    CallSignature(FunctionSignatureFact),
    /// A construct signature.
    ConstructSignature(FunctionSignatureFact),
    /// An index signature.
    IndexSignature(IndexSignatureFact),
}

/// The `PreparedValueDecl.object_shape: Option<ObjectExpr>` narrowing — closed
/// over all five member variants.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct ObjectShapeFact {
    /// The ordered object members.
    pub members: Arc<[ObjectMemberFact]>,
}

/// A folded/sound enum member scalar. Numeric values are stored as the
/// CANONICAL `f64` display string — `format!("{value}")` of the folded value
/// (see `format_enum_number` in `verter_semantic::analysis::type_eval_build`),
/// never a raw `f64` (which would break `Eq`/`Hash` identity) and never the
/// verbatim source spelling. The fingerprint producer
/// (`scalar_to_type_expr` in `verter_semantic::facts::hashing`) parses the
/// string back to the exact same bits, so string-identity dedup on this fact
/// is equivalent to `f64::to_bits` dedup and the byte-parity contract between
/// the parse-time and lowering-time emitters holds.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum EnumScalar {
    /// A folded numeric literal, as the canonical `f64` display string
    /// (`format!("{value}")` — round-trips to exact bits).
    Number(String),
    /// A folded string literal.
    String(String),
    /// A sound primitive domain (an unfolded computed member).
    Primitive(EnumPrimitiveDomain),
}

/// The sound primitive domain of an unfolded computed enum member.
///
/// The four arms are the exact codomain of the deferred-member domain
/// classifier: a numeric-yielding initializer is [`Number`](Self::Number), a
/// string-yielding one is [`String`](Self::String), a `+` initializer (numeric
/// add OR string concat — the soundest bound is the union of both) is
/// [`NumberOrString`](Self::NumberOrString), and an unprovable initializer
/// (member reference, call, boolean-yielding operator, anything else) is
/// [`Unknown`](Self::Unknown). `Number | String` alone cannot faithfully encode
/// the last two, so a deferred `+` member and a genuinely unprovable member would
/// otherwise be forced to a wrong narrower domain.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum EnumPrimitiveDomain {
    /// A numeric enum member domain.
    Number,
    /// A string enum member domain.
    String,
    /// A `number | string` domain — a `+` initializer is numeric add OR string
    /// concat, so the soundest bound is the union of both.
    NumberOrString,
    /// An unprovable member domain (a member-reference / call / boolean-yielding
    /// operator / any non-literal initializer) — no narrower sound domain exists.
    Unknown,
}

/// One ordered enum member (name → closed scalar).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct EnumMemberEntry {
    /// The member name.
    pub name: String,
    /// The member's folded/sound value.
    pub value: EnumScalar,
}

/// The `PreparedValueDecl.enum_members` narrowing — the ordered inventory.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct EnumMemberFact {
    /// The ordered enum members.
    pub members: Arc<[EnumMemberEntry]>,
}

/// The `PreparedMember.ty` narrowing. `PreparedMember.spans` is recovered via
/// `span_origin`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct PreparedMemberFact {
    /// Whether the member is optional.
    pub optional: bool,
    /// Whether the member is readonly.
    pub readonly: bool,
    /// Whether the member is a method.
    pub is_method: bool,
    /// Member visibility.
    pub visibility: MemberVisibility,
    /// Typed declaration origin (defining file / synthetic).
    pub declaration_origin: DeclarationOrigin,
    /// The member type body locator.
    pub ty: TypeBodySlot,
    /// Origin locator recovering the member's `MemberSpans`.
    pub span_origin: MemberSpansOrigin,
}

/// The `PreparedValueMember.ty` narrowing (+ `is_method`).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct PreparedValueMemberFact {
    /// Whether the value member is a method.
    pub is_method: bool,
    /// The member type body locator.
    pub ty: TypeBodySlot,
}

/// Case-transform kind for a key remap (lower-neutral copy of the prep kind).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum PreparedCaseTransformKind {
    /// `Capitalize<K>`.
    Capitalize,
    /// `Uncapitalize<K>`.
    Uncapitalize,
    /// `Uppercase<K>`.
    Uppercase,
    /// `Lowercase<K>`.
    Lowercase,
}

/// The narrowed `PreparedKeyFilterShape` — `Opaque(TypeExpr)` becomes a locator.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum PreparedKeyFilterShapeFact {
    /// No key filter.
    All,
    /// Include exactly these literal keys.
    IncludeLiteral(Arc<[String]>),
    /// Exclude exactly these literal keys.
    ExcludeLiteral(Arc<[String]>),
    /// A non-literal key filter addressed by locator.
    Opaque(TypeBodySlot),
}

/// The narrowed `PreparedKeyRemapShape` — `Opaque(TypeExpr)` becomes a locator.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum PreparedKeyRemapShapeFact {
    /// Identity remap.
    Identity,
    /// Prefix each key.
    Prefix(String),
    /// Suffix each key.
    Suffix(String),
    /// Case-transform each key.
    CaseTransform(PreparedCaseTransformKind),
    /// A non-literal key remap addressed by locator.
    Opaque(TypeBodySlot),
}

/// The narrowed `PreparedValueRuleShape` — `Transform(TypeExpr)` becomes a
/// locator.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum PreparedValueRuleShapeFact {
    /// The value passes through unchanged.
    PassThrough,
    /// The value is transformed via the located type.
    Transform(TypeBodySlot),
}

/// Forwarding kind for a forward-subject projection.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum PreparedForwardingKind {
    /// Identity type parameters are forwarded unchanged.
    IdentityParams,
    /// An applied alias forwards concrete arguments.
    AppliedAlias,
}

/// The narrowed `PreparedForwardPayload` — `target_args: Vec<TypeExpr>` becomes
/// `Arc<[TypeArgLocator]>` (keeping `target_name` + `forwarding_kind`).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct PreparedForwardPayloadFact {
    /// The forwarded target name.
    pub target_name: String,
    /// The forwarding kind.
    pub forwarding_kind: PreparedForwardingKind,
    /// The forwarded type arguments as locators.
    pub target_args: Arc<[TypeArgLocator]>,
}

/// The structural wrapper classification discriminant (`PreparedWrapperKind`) —
/// a 1:1 lower-neutral copy (no `TypeExpr`).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum PreparedWrapperKindFact {
    /// Not a wrapper.
    None,
    /// The identity wrapper (`{ [K in keyof T]: T[K] }`).
    Identity,
    /// A pure modifier overlay (`Partial`/`Required`/`Readonly`/`Mutable`).
    PureOverlay,
    /// A key-filtering wrapper (`Pick`/`Omit`).
    KeyFilter,
    /// A key-remapping wrapper (`as`-clause mapped type).
    KeyRemap,
}

/// The narrowed surface modifiers (`PreparedSurfaceModifiers`) — `+`/`-` optional
/// and readonly overlays. A `None` field means "unchanged".
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct PreparedSurfaceModifiersFact {
    /// Optionality overlay (`Some(true)` add `?`, `Some(false)` remove, `None`
    /// unchanged).
    pub optional: Option<bool>,
    /// Readonly overlay (same tri-state semantics).
    pub readonly: Option<bool>,
}

/// The narrowed `PreparedWrapperShape` — the full structural wrapper
/// classification. Every `TypeExpr`-bearing sub-shape (`key_filter` / `key_remap`
/// / `value_rule`) is already narrowed to its `*Fact` (opaque payloads become
/// locators); no field is left as vague bundle prose.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct PreparedWrapperShapeFact {
    /// The wrapper kind discriminant.
    pub kind: PreparedWrapperKindFact,
    /// The mapped/source type-parameter index this wrapper keys off, if any.
    pub source_param_index: Option<u16>,
    /// The key-filter shape (`Pick`/`Omit` domain; opaque → locator).
    pub key_filter: PreparedKeyFilterShapeFact,
    /// The key-remap shape (`as`-clause remap; opaque → locator).
    pub key_remap: PreparedKeyRemapShapeFact,
    /// The value-transform rule (opaque transform → locator).
    pub value_rule: PreparedValueRuleShapeFact,
    /// The optional/readonly modifier overlays.
    pub modifiers: PreparedSurfaceModifiersFact,
}

/// The narrowed `PreparedProjectionClass` — the top-level projection strategy.
/// The `Wrapper` details live on the owning decl's `PreparedWrapperShapeFact`;
/// `ForwardSubject` carries the forward payload fact.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum PreparedProjectionClassFact {
    /// The decl projects its own directly-declared members.
    DirectMembers,
    /// The decl is a structural wrapper (details on `PreparedWrapperShapeFact`).
    Wrapper,
    /// The decl forwards to another target (payload carried here).
    ForwardSubject(PreparedForwardPayloadFact),
    /// An opaque projection (no cheap structural classification).
    Opaque,
}

// ===========================================================================
// Analyzed* / Projected* / synthesized facts (the [P2] named-instance surface)
// ===========================================================================

/// The structural macro role (from `AnalyzedMacro.kind` — the type-role
/// authority; role classification is structural, never nominal).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum AnalyzedMacroKindFact {
    /// `defineProps`.
    DefineProps,
    /// `defineEmits`.
    DefineEmits,
    /// `defineSlots`.
    DefineSlots,
    /// `defineOptions`.
    DefineOptions,
    /// `defineExpose`.
    DefineExpose,
    /// `defineModel`.
    DefineModel,
    /// `withDefaults`.
    WithDefaults,
}

/// The narrowed `AnalyzedPropField`. The authored `type_expr` becomes a payload
/// locator; the prop-name span is recovered via `name_span_origin`. Display-only
/// fields (`type_annotation`, `description`, `tags`, `resolution_error`) are
/// carve-outs, not stored here.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct AnalyzedPropFieldFact {
    /// The prop key.
    pub name: String,
    /// Whether the prop is optional.
    pub is_optional: bool,
    /// Whether the prop was author-declared in the macro type argument (vs
    /// heritage-derived). Policy-consumed at publication (NOT display).
    pub declared_in_macro_type_arg: bool,
    /// The scope-pairing (producing canonical of the narrowed payload body).
    pub type_expr_scope: Option<TypeExprScope>,
    /// The narrowed prop type body locator.
    pub payload: Option<MacroPayloadLocator>,
    /// Origin locator recovering the prop-name span.
    pub name_span_origin: MemberSpansOrigin,
}

/// The narrowed `AnalyzedEmitField`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct AnalyzedEmitFieldFact {
    /// The event key.
    pub name: String,
    /// The scope pairing.
    pub type_expr_scope: Option<TypeExprScope>,
    /// The narrowed emit-signature payload locator.
    pub payload: Option<MacroPayloadLocator>,
    /// Origin locator recovering the emit-name span.
    pub name_span_origin: MemberSpansOrigin,
}

/// The narrowed `AnalyzedSlotFieldBinding`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct AnalyzedSlotFieldBindingFact {
    /// The binding key.
    pub name: String,
    /// The scope pairing.
    pub type_expr_scope: Option<TypeExprScope>,
    /// The narrowed binding payload locator (typically an indexed access).
    pub payload: Option<MacroPayloadLocator>,
    /// Origin locator recovering the binding-name span.
    pub name_span_origin: MemberSpansOrigin,
}

/// The narrowed `AnalyzedSlotField`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct AnalyzedSlotFieldFact {
    /// The slot key.
    pub name: String,
    /// Whether the slot is required (`optional` inverse).
    pub is_required: bool,
    /// The scope pairing.
    pub type_expr_scope: Option<TypeExprScope>,
    /// The narrowed slot return payload locator.
    pub payload: Option<MacroPayloadLocator>,
    /// The slot bindings.
    pub bindings: Arc<[AnalyzedSlotFieldBindingFact]>,
    /// Origin locator recovering the slot-name span.
    pub name_span_origin: MemberSpansOrigin,
}

/// The narrowed `AnalyzedOptionsProp` (Options API).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct AnalyzedOptionsPropFact {
    /// The prop key.
    pub name: String,
    /// Whether the prop is required.
    pub is_required: bool,
    /// Whether the prop has a default.
    pub has_default: bool,
    /// The Vue runtime constructor name (`String`, `Number`, …), if present.
    pub type_constructor: Option<String>,
    /// The scope pairing.
    pub type_expr_scope: Option<TypeExprScope>,
    /// The narrowed `PropType<T>` payload locator.
    pub payload: Option<MacroPayloadLocator>,
    /// Origin locator recovering the prop-name span.
    pub name_span_origin: MemberSpansOrigin,
}

/// The narrowed `AnalyzedExposeField`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct AnalyzedExposeFieldFact {
    /// The expose key.
    pub name: String,
    /// The scope pairing.
    pub type_expr_scope: Option<TypeExprScope>,
    /// The narrowed expose payload locator.
    pub payload: Option<MacroPayloadLocator>,
    /// Origin locator recovering the expose-name span (synthetic for type-arg
    /// surface members with no authored object-literal span).
    pub name_span_origin: MemberSpansOrigin,
}

/// The narrowed `AnalyzedMacro` (incl. `parsed_type_argument`).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct AnalyzedMacroFact {
    /// The structural macro role (the type-role authority).
    pub kind: AnalyzedMacroKindFact,
    /// Whether the macro is type-based.
    pub is_type_based: bool,
    /// Whether the macro declares `inheritAttrs: false`.
    pub has_inherit_attrs_false: bool,
    /// Referenced type names.
    pub type_references: Arc<[String]>,
    /// The `defineModel` name, if any.
    pub model_name: Option<String>,
    /// The narrowed `parsed_type_argument` parent-shell payload locator.
    pub parsed_type_argument: Option<MacroPayloadLocator>,
    /// The scope pairing for `parsed_type_argument`.
    pub parsed_type_argument_scope: Option<TypeExprScope>,
}

/// One synthesized LEAF member of a fabricated depth-closed sub-surface: a
/// name, an optionality bit, and a closed LEAF value. The value is a
/// [`LeafTypeFact`] — never a [`FactOrLocator`] — so the
/// [`FactOrLocator::LeafObject`] arm stays closed and finite (no open
/// recursive arm, per this module's schema law). The member is fabricated by
/// construction (a synthesized component default's legacy-prop / snippet-slot
/// map entry), so no span origin is carried.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct SynthesizedLeafMember {
    /// The member name.
    pub name: String,
    /// Whether the member is optional.
    pub optional: bool,
    /// The closed leaf member value.
    pub ty: LeafTypeFact,
}

/// The `ty` of a synthesized member: a closed scalar/leaf fact OR a locator
/// escape. There is NO open recursive `Box<Self>` arm — any non-leaf structure
/// is a locator, except the two depth-closed arms whose interiors are LEAVES
/// (not `FactOrLocator`, so recursion is impossible): the finite leaf-union
/// arm and the leaf-member sub-object arm.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum FactOrLocator {
    /// A closed scalar/leaf fact.
    Leaf(LeafTypeFact),
    /// A closed, finite UNION of leaves in a member/element/param position
    /// (`string | number` as a realized emit payload tuple element) — the
    /// nested mirror of [`ClosedTypeFact::LeafUnion`]. Ordered as produced;
    /// leaf members only, so the arm stays non-recursive and the fact is
    /// complete by itself (never a union of unions, never a locator arm).
    LeafUnion(Arc<[LeafTypeFact]>),
    /// The escape: any non-leaf structure addressed by a body locator.
    Locator(TypeBodySlot),
    /// The authored macro / field payload escape (`define*<T>()`, `$props<T>()`,
    /// `createEventDispatcher<E>()`): the attachment point for a REAL authored
    /// payload consumed by a fabricated surface — a synthesized component
    /// default's `$props` / `$emit` / `$slots` / `$events` member values. The
    /// payload lowers on demand through the one shared dispatch (the
    /// macro-payload locator deref / hot-mirror producer), never eagerly.
    MacroPayload(MacroPayloadLocator),
    /// A fabricated depth-closed sub-object surface (a synthesized component
    /// default's empty / legacy-prop `$props` map and its snippet `$slots`
    /// map): named LEAF members only — closed and finite by construction,
    /// never an open recursive arm.
    LeafObject(Arc<[SynthesizedLeafMember]>),
}

/// A closed leaf type fact (a primitive, a literal, or a bare named reference).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum LeafTypeFact {
    /// A primitive type.
    Primitive(PrimitiveName),
    /// A string literal.
    StringLiteral(String),
    /// A numeric literal (exact source repr).
    NumberLiteral(String),
    /// A boolean literal.
    BooleanLiteral(bool),
    /// A bare named reference (shallow — resolved elsewhere on demand).
    Ref(String),
}

/// One synthesized object member (the [P1] synthesized-(d) schema). `readonly =
/// false` and `visibility = Public` are producer-constants (not stored). The
/// member span is recovered via `span_origin`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct SynthesizedMemberFact {
    /// The member name.
    pub name: String,
    /// Whether the member is optional.
    pub optional: bool,
    /// The member type (fact-or-locator).
    pub ty: FactOrLocator,
    /// Origin locator recovering `MemberSpans::name_only(field.span)`.
    pub span_origin: MemberSpansOrigin,
}

/// One tuple element fact.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct TupleElementFact {
    /// The element label, if a named tuple member.
    pub label: Option<String>,
    /// Whether the element is optional.
    pub optional: bool,
    /// Whether the element is a rest element.
    pub rest: bool,
    /// The element type (fact-or-locator).
    pub ty: FactOrLocator,
}

/// A tuple payload fact.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct TuplePayloadFact {
    /// Whether the tuple is readonly.
    pub readonly: bool,
    /// The ordered tuple elements.
    pub elements: Arc<[TupleElementFact]>,
}

/// An indexed-access fact (`Obj['a']['b']`) — path-precise, graph-free.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct IndexedAccessFact {
    /// The object body locator.
    pub object: TypeBodySlot,
    /// The ordered index-key path.
    pub index_path: Arc<[String]>,
}

/// The synthesized shape of a `ResolvedLocalType`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum ResolvedLocalShape {
    /// A synthesized object surface.
    Object(Arc<[SynthesizedMemberFact]>),
    /// A tuple payload.
    Tuple(TuplePayloadFact),
    /// An indexed access.
    IndexedAccess(IndexedAccessFact),
    /// A single leaf type.
    Leaf(LeafTypeFact),
    /// A bare reference resolved elsewhere (shallow).
    Ref(SymbolBodyLocator),
}

/// The `ResolvedLocalType` → `ResolvedLocalTypeFact` narrowing (synthesized-(d)).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct ResolvedLocalTypeFact {
    /// The type name as referenced.
    pub name: String,
    /// The synthesized shape.
    pub shape: ResolvedLocalShape,
}

/// The narrowed `ProjectedMember`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct ProjectedMemberFact {
    /// The member name.
    pub name: String,
    /// Whether the member is optional.
    pub optional: bool,
    /// Whether the member is readonly.
    pub readonly: bool,
    /// Whether the member is a method.
    pub is_method: bool,
    /// Member visibility.
    pub visibility: MemberVisibility,
    /// Whether the member was author-declared in the macro type argument.
    pub declared_in_macro_type_arg: bool,
    /// The typed declaration origin.
    pub declaration_origin: DeclarationOrigin,
    /// The member value type body locator.
    pub ty: TypeBodySlot,
    /// Origin locator recovering the member's `MemberSpans`.
    pub span_origin: MemberSpansOrigin,
}

/// The narrowed `ProjectedIndexSignature`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct ProjectedIndexSignatureFact {
    /// The index key parameter name.
    pub key_name: String,
    /// The declared key SHAPE.
    pub key_type: KeyTypeShape,
    /// The value type body locator.
    pub value_type: TypeBodySlot,
    /// Whether the index signature is readonly.
    pub readonly: bool,
    /// The typed declaration origin.
    pub declaration_origin: DeclarationOrigin,
    /// Origin locator recovering the `IndexSignatureSpans`.
    pub span_origin: IndexSignatureSpansOrigin,
}

/// The narrowed `ProjectedSurface`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct ProjectedSurfaceFact {
    /// The projected members.
    pub members: Arc<[ProjectedMemberFact]>,
    /// Ordered call signatures.
    pub call_signatures: Arc<[FunctionSignatureFact]>,
    /// Ordered construct signatures.
    pub construct_signatures: Arc<[FunctionSignatureFact]>,
    /// Concrete declared index signatures.
    pub index_signatures: Arc<[ProjectedIndexSignatureFact]>,
    /// Open-surface flag (distinct from concrete `index_signatures`).
    pub has_index_signature: bool,
}

// ===========================================================================
// Svelte facts
// ===========================================================================

/// One Svelte legacy prop (`export let`) fact.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct SvelteLegacyPropFact {
    /// The prop name.
    pub name: String,
    /// Whether the prop has a default (optionality).
    pub has_default: bool,
}

/// The narrowed persisted `SvelteScriptFacts`. `props_type` /
/// `dispatcher_events` are authored-type payload refs: a content-free
/// `MacroPayload` locator (the re-resolution address) plus a parse-stable
/// structural payload hash (the cache discriminator). Inline object literals,
/// instantiations carrying type arguments, and bare named references ALL
/// carry a payload ref — never a raw `TypeExpr`, never fail-closed.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct SvelteScriptFactsFact {
    /// The props authored-type payload ref (shallow-by-default).
    pub props_type: Option<AuthoredTypePayloadRef>,
    /// MODEL binding names.
    pub bindable_members: Arc<[String]>,
    /// svelte-package-validated snippet prop names.
    pub validated_snippet_members: Arc<[String]>,
    /// Legacy props.
    pub legacy_props: Arc<[SvelteLegacyPropFact]>,
    /// The `createEventDispatcher<E>()` type-arg authored-type payload ref,
    /// when provenance-validated.
    pub dispatcher_events: Option<AuthoredTypePayloadRef>,
    /// EXPOSE surface (instance exports).
    pub instance_exports: Arc<[String]>,
}

// ===========================================================================
// Four-source `SemanticTypeSource` — the reusable NoTypeExpr replacement for a
// resolved/evaluated `TypeExpr` field position (the `Expanded*` `ty` positions
// and the component-meta `*Analysis` resolved/evaluated positions).
// ===========================================================================

/// A closed leaf/object/function/tuple/indexed-access type fact — the (d)
/// "lower leaf/object/function/tuple" source composed from the already-defined
/// closed families. Every arm is a closed fact (never a `TypeExpr`); non-leaf
/// interior structure of a member/element is a locator inside those facts.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum ClosedTypeFact {
    /// A primitive / literal / bare-named leaf.
    Leaf(LeafTypeFact),
    /// A closed, finite UNION of leaves (`"a" | "c"`, `1 | 2`) — the decided
    /// result of a fully-closed reduction (e.g. a distributive
    /// `Exclude`/`Extract` over literal unions). Ordered as produced; leaf
    /// members only, so the fact is complete by itself.
    LeafUnion(Arc<[LeafTypeFact]>),
    /// A closed object surface.
    Object(ObjectShapeFact),
    /// A function / call-shape.
    Function(FunctionSignatureFact),
    /// A tuple payload.
    Tuple(TuplePayloadFact),
    /// A path-precise indexed access (`Obj['a']['b']`).
    IndexedAccess(IndexedAccessFact),
}

/// A projected-surface type fact — the (c-shaped) "projected fact" source
/// composed from the already-defined `Projected*` families. A resolved member /
/// index-signature / call-or-construct signature / whole projected surface.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum ProjectedTypeFact {
    /// A projected object property member.
    Member(ProjectedMemberFact),
    /// A projected index signature.
    IndexSignature(ProjectedIndexSignatureFact),
    /// A projected call signature.
    CallSignature(FunctionSignatureFact),
    /// A projected construct signature.
    ConstructSignature(FunctionSignatureFact),
    /// A whole projected surface.
    Surface(ProjectedSurfaceFact),
    /// A projected MEMBER-PATH route: the content-free replay address for a
    /// member the publication surface materialized through path projection
    /// off an authored base body — merged same-name members (duplicate /
    /// intersection contributors), inherited referenced tuples / objects,
    /// and substituted generic surfaces, none of which a single contributor
    /// locator or a closed fact can faithfully express. `base` is the
    /// authored body the projection starts from (for a macro payload member:
    /// the macro's STAMPED type-argument locator); `path` is the ordered
    /// member-name hop chain (for a property-style emit: the event name).
    /// Raising it replays the base plus the path through the one shared
    /// dispatch's EXISTING `ProjectPath` query — never a second resolver,
    /// never a stored `TypeExpr` or graph node id.
    MemberPath {
        /// The authored base body the projection starts from.
        base: AuthoredBodyLocator,
        /// The ordered member-name path off the base.
        path: Arc<[String]>,
    },
    /// A projected CALLABLE-PARAMS route: the content-free replay address
    /// for a realized call-signature payload tuple the publication surface
    /// synthesized from the signature's parameters — parameters richer than
    /// the closed leaf / leaf-union element vocabulary (cross-file
    /// references, composites, nested objects, arrays, callbacks,
    /// instantiated generics), which no closed fact and no single
    /// contributor locator can faithfully express. `base` is the authored
    /// body whose projected surface carries the call signature (for a macro
    /// payload: the macro's STAMPED type-argument locator);
    /// `signature_ordinal` indexes the projected surface's call-signature
    /// sequence (declaration order, BEFORE any event-name expansion /
    /// deduplication); `first_param` is the index of the first PAYLOAD
    /// parameter (a Vue emit signature strips the leading event-name
    /// parameter, so its rows stamp `1`). Raising it replays the base's
    /// surface projection, selects the signature at the ordinal in the node
    /// domain, and synthesizes a TRANSIENT tuple from the signature's raw
    /// parameters — labels / optionality / rest / order / nesting /
    /// generic substitutions preserved — through the one shared dispatch;
    /// never a second resolver, never a stored `TypeExpr` or graph node id.
    /// Bounds drift, a missing surface, a non-callable ordinal, a
    /// `first_param` past the parameter list, or an unresolvable payload
    /// parameter FAILS the raise honestly — never an empty-tuple or
    /// fabricated-element synthesis.
    CallableParams {
        /// The authored base body whose projected surface carries the call
        /// signature.
        base: AuthoredBodyLocator,
        /// The call signature's ordinal in the projected surface's
        /// call-signature sequence (declaration order, pre-expansion).
        signature_ordinal: u32,
        /// The first PAYLOAD parameter index (parameters before it are
        /// address/name parameters, not payload).
        first_param: u32,
    },
    /// A projected INDEX-POSITION route: the content-free replay address for
    /// an index signature's KEY or VALUE type position on the publication
    /// surface (`{ [key: string]: { nested: number } }`) — positions richer
    /// than the closed leaf / tuple vocabulary (nested objects, functions,
    /// composites, references), which no closed fact can faithfully express
    /// and which carry NO member name a [`Self::MemberPath`] hop could
    /// address. `base` is the authored body whose projected surface carries
    /// the index signature (for a macro payload: the macro's STAMPED
    /// type-argument locator); `signature_ordinal` indexes the projected
    /// surface's index-signature sequence (declaration order — the exact
    /// sequence the publication normalizer enumerated); `position` selects
    /// the key or the value type. Raising it replays the base's surface
    /// projection, selects the signature at the ordinal in the node domain,
    /// and hands back the position's own (possibly substituted) node through
    /// the one shared dispatch — never a second resolver, never a stored
    /// `TypeExpr` or graph node id. Bounds drift, a missing surface, or an
    /// unknown-materializing position node FAILS the raise honestly — never
    /// a fabricated body.
    IndexPosition {
        /// The authored base body whose projected surface carries the index
        /// signature.
        base: AuthoredBodyLocator,
        /// The index signature's ordinal in the projected surface's
        /// index-signature sequence (declaration order).
        signature_ordinal: u32,
        /// Which type position of the signature this addresses.
        position: IndexSignaturePosition,
    },
}

/// The addressed type position of a projected index signature — the KEY
/// (`[key: K]`) or the VALUE (`: V`) slot of
/// [`ProjectedTypeFact::IndexPosition`].
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum IndexSignaturePosition {
    /// The declared key type (`[key: K]`).
    Key,
    /// The declared value type (`: V`).
    Value,
}

/// The synthesized-(d) source shape carried in a
/// [`SemanticTypeSource::Synthesized`] position. This is EXACTLY the closed
/// synthesized shape already defined as [`ResolvedLocalShape`] (object members
/// via [`SynthesizedMemberFact`], tuple via [`TuplePayloadFact`], indexed-access
/// via [`IndexedAccessFact`], leaf via [`LeafTypeFact`], shallow `Ref` via
/// [`SymbolBodyLocator`]). It is REUSED as a type alias — NOT duplicated — so the
/// synthesized-source shape and the `ResolvedLocalType` shape can never diverge
/// (a single closed schema, one place to add an arm).
///
/// [`SymbolBodyLocator`]: crate::locators::SymbolBodyLocator
pub type SynthesizedTypeFact = ResolvedLocalShape;

/// The reusable four-source closed replacement for a resolved/evaluated type
/// field position. Every semantic type reached at a resolved surface is ONE of
/// four disjoint sources (design four-source model):
///
/// - [`Authored`](Self::Authored): an authored parse-backed body → the
///   content-free [`AuthoredBodyLocator`] (the single graph-engine-routed
///   escape).
/// - [`Projected`](Self::Projected): a projected-surface fact.
/// - [`Synthesized`](Self::Synthesized): a synthesized closed shape (the
///   `ResolvedLocalType`-style source).
/// - [`Closed`](Self::Closed): a directly-closed leaf/object/function/tuple/
///   indexed-access fact.
/// - [`SyntheticSlotBinding`](Self::SyntheticSlotBinding): a first-class
///   synthetic slot-binding / `defineSlots` binding carrier source — the
///   publication source for a graph-native binding row with no parser-side
///   payload. The [`crate::SyntheticCarrierKey`] mirrors the peer typed-IR
///   carrier [`crate::TypeExpr::SyntheticSlotBinding`]; its `value_node`
///   arena ordinal is value-side provenance for the same-generation
///   explicit-deepen seed, NEVER a cache identity (the content-free
///   deepen identity is the session-side `SyntheticBindingId` projection).
///
/// The session/framework-adapter-raised (c) source is NOT an arm here: it is held
/// session-side as a `SessionDemandIdentity` and never stored in a lower crate —
/// the lower-neutral carrier of a (c) position is an authored locator + a
/// producing-canonical scope pairing, so it lands on [`Authored`](Self::Authored)
/// / a fact, never a session-typed replay identity.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum SemanticTypeSource {
    /// (a) an authored parse-backed body addressed by a content-free locator.
    Authored(AuthoredBodyLocator),
    /// (c-projected) a projected-surface fact.
    Projected(ProjectedTypeFact),
    /// (d) a synthesized closed shape.
    Synthesized(SynthesizedTypeFact),
    /// (d-closed) a directly-closed leaf/object/function/tuple/indexed-access
    /// fact.
    Closed(ClosedTypeFact),
    /// (s) a synthetic slot-binding / `defineSlots` binding carrier source
    /// (the no-parser graph-native binding row's publication source).
    SyntheticSlotBinding(Arc<crate::SyntheticCarrierKey>),
}

/// A PROVEN structural reason a schema position carries no semantic type
/// source. Every arm is a specific structural fact about the producing
/// schema — there is deliberately NO generic `Missing` catch-all: a position
/// that lacks a source WITHOUT a proven absence reason is a
/// [`SemanticSourceFailure`], never an absence.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum SchemaAbsence {
    /// The position carries no semantic type annotation in the producing
    /// schema: an untyped/inferred position (an array-form emit, a runtime
    /// prop with no type, an untyped binding, a display-only intrinsic
    /// catalog shape). Rendering the canonical typed `unknown` for it is
    /// honest absence, not a masked failure.
    Unannotated,
    /// A cross-branch merged accepted/fallthrough row whose per-branch typed
    /// sources DIVERGED (two branches carried distinct source identities):
    /// the merged row has no single representable source, proven by the
    /// branch fold itself.
    BranchDivergent,
}

/// A typed source-construction failure at a REQUIRED source position. The
/// producer could not build a faithful [`SemanticTypeSource`] for a position
/// the schema REQUIRES to have one; encoding this as a `Closed(Leaf(unknown))`
/// success is forbidden — the failure must fail output materialization
/// instead of rendering as a completed `unknown`.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum SemanticSourceFailure {
    /// A REQUIRED payload/value position (a realized emit signature's payload
    /// tuple, a type-based macro member's value type) could not be
    /// represented in the available source vocabulary.
    UnrepresentableRequiredPayload,
    /// A REQUIRED published MEMBER-VALUE position (a type-based macro
    /// surface member's value type, an index-signature key/value position)
    /// could not be represented in the available source vocabulary: no
    /// authored slot, no use-site slot, no reference identity, no closed
    /// fact. Covers the whole no-faithful-source residue — a partial
    /// materialization, an unknown-materializing resolver failure carrier,
    /// an unresolved residual reference/import/typeof carrier, AND a
    /// reached-but-unencodable structural value (a function / object /
    /// non-empty tuple / composite): known structure without a faithful
    /// source is still a FAILURE, never a fabricated `unknown` success.
    UnrepresentableRequiredMemberValue,
}

/// The three-state SOURCE POSITION of a resolved/published type field: a
/// proven schema ABSENCE, a PRESENT four-source value, or a typed
/// source-construction FAILURE. The three states are semantically DISJOINT:
///
/// - [`Absent`](Self::Absent) — the schema position has no semantic source
///   for a PROVEN structural reason; consumers render the centralized typed
///   `unknown` and the result stays a valid success.
/// - [`Present`](Self::Present) — a faithful [`SemanticTypeSource`]; an
///   authored/open `unknown` value is a PRESENT success
///   (`Present(Closed(Leaf(unknown)))`), never an absence and never a
///   failure.
/// - [`Failed`](Self::Failed) — a REQUIRED position whose source could not
///   be constructed; output materialization FAILS (a typed error), the
///   result is non-complete, and it is never cached as a complete success.
///
/// A consumer must never collapse `Failed` into `Absent` (which renders
/// `unknown`-as-success) or into a fabricated `Present` value — that
/// collapse is the fail-open this carrier eliminates.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum SourcePosition {
    /// The schema position PROVABLY carries no semantic source.
    Absent(SchemaAbsence),
    /// A faithful present source.
    Present(SemanticTypeSource),
    /// A REQUIRED position whose source construction failed.
    Failed(SemanticSourceFailure),
}

impl SourcePosition {
    /// The canonical unannotated schema absence.
    #[must_use]
    pub fn unannotated() -> Self {
        SourcePosition::Absent(SchemaAbsence::Unannotated)
    }

    /// The present source, when this position carries one. `Absent` and
    /// `Failed` positions expose NO source — a `present()`-only consumer can
    /// never mistake either state for a value.
    #[must_use]
    pub fn present(&self) -> Option<&SemanticTypeSource> {
        match self {
            SourcePosition::Present(source) => Some(source),
            SourcePosition::Absent(_) | SourcePosition::Failed(_) => None,
        }
    }

    /// Consume the position and yield the present source, when any.
    #[must_use]
    pub fn into_present(self) -> Option<SemanticTypeSource> {
        match self {
            SourcePosition::Present(source) => Some(source),
            SourcePosition::Absent(_) | SourcePosition::Failed(_) => None,
        }
    }

    /// Mutable access to the present source, when this position carries one.
    #[must_use]
    pub fn present_mut(&mut self) -> Option<&mut SemanticTypeSource> {
        match self {
            SourcePosition::Present(source) => Some(source),
            SourcePosition::Absent(_) | SourcePosition::Failed(_) => None,
        }
    }

    /// Whether this position carries a present source.
    #[must_use]
    pub fn is_present(&self) -> bool {
        matches!(self, SourcePosition::Present(_))
    }

    /// Whether this position is a typed source-construction failure.
    #[must_use]
    pub fn is_failed(&self) -> bool {
        matches!(self, SourcePosition::Failed(_))
    }
}

/// Lower-neutral copy of the solver exactness metadata — the semantic exactness
/// of an expansion result, carried as fact metadata. `verter_type_expr` is below
/// `verter_semantic`, so the session-side `SolverExactness` cannot be named here;
/// this is the graph-free copy the fact substrate carries.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum ExpansionExactnessFact {
    /// A fully materialized finite result.
    ExactConcrete,
    /// Exact but not finitely materialized (open mapped types, recursive type
    /// identities, infinite-keyspace symbolic forms).
    ExactSymbolic,
    /// Missing source, unsupported syntax, cancelled request, or a hard
    /// recursion-policy stop.
    Incomplete,
}

/// Lower-neutral copy of the solver execution-status metadata — the operational
/// status of an expansion, carried as fact metadata (tracked separately from
/// semantic exactness so operational interruption is never modeled as a semantic
/// approximation).
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub enum ExpansionExecutionStatusFact {
    /// Completed normally within all operational guards.
    Completed,
    /// Cancelled by the caller (e.g. request timeout).
    Cancelled,
    /// Interrupted by an operational guard (e.g. instantiation depth).
    Interrupted,
    /// Hit a deterministic hard stop (e.g. template-literal explosion).
    HardStop,
}

/// The lower-neutral wrapper for an `Expanded*` `result` position — the payload
/// fact `T` plus lower-neutral expansion metadata. Generic over the payload fact
/// `T` (which must itself be a fact carrier: `NoTypeExpr + NoStoredSpan + Eq +
/// Hash`); the derived witnesses forward the bound to `T`, so
/// `ExpansionResultFact<T>` is a fact carrier iff `T` is (a raw-`TypeExpr`
/// payload is rejected — see the negative witness in `fact_witnesses`).
///
/// Diagnostics are deliberately NOT carried: they are observability / output
/// facts, not semantic type authority, so they never enter the content-free fact
/// substrate. Exactness and execution status ARE fact metadata (they gate
/// exact/complete decisions and warm admission).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    NoTypeExpr,
    NoStoredSpan,
)]
pub struct ExpansionResultFact<T> {
    /// The expansion result payload fact.
    pub value: T,
    /// Semantic exactness of the result.
    pub exactness: ExpansionExactnessFact,
    /// Operational execution status of the result.
    pub execution_status: ExpansionExecutionStatusFact,
}

// ===========================================================================
// Producer-local anchor absolutization (cross-owner self-anchoring)
// ===========================================================================
//
// Deep-walk companions of `AuthoredBodyLocator::absolutized_against` (see
// `locators.rs`): every anchor-bearing position inside the fact families
// rewrites a producer-local (empty-`canonical_id`) anchor to the supplied
// owning canonical, so a source cloned across an owner boundary (fallthrough
// inheritance) stays SELF-ANCHORING instead of silently re-anchoring to the
// consuming scope's file. Already-absolute anchors are never rewritten, and
// the anchor-free closed leaf family (`Leaf` / `LeafUnion` / `LeafObject`)
// passes through untouched — a published child-local alias name stays the
// name AS WRITTEN. Each private `absolutize` returns `None` when nothing
// needed rewriting so unchanged subtrees keep their shared allocations.

/// Rebuild an `Arc` slice only when at least one element changed.
///
/// Copy-on-first-change: the scan performs NO clones and NO allocation
/// until the first element that actually needs rewriting; the dominant
/// already-absolute case (a source re-absolutized under a consuming scope)
/// returns `None` allocation-free. On the first change the unchanged
/// prefix is copied once and the remainder rebuilt.
fn absolutize_fact_slice<T: Clone>(
    items: &Arc<[T]>,
    absolutize: impl Fn(&T) -> Option<T>,
) -> Option<Arc<[T]>> {
    // Allocation-free scan to the first changed element; an unchanged
    // slice exits here without touching the heap.
    let (first_changed, first_value) = items
        .iter()
        .enumerate()
        .find_map(|(index, item)| absolutize(item).map(|next| (index, next)))?;
    // Copy the unchanged prefix once, then rebuild the rest.
    let mut rebuilt: Vec<T> = Vec::with_capacity(items.len());
    rebuilt.extend_from_slice(&items[..first_changed]);
    rebuilt.push(first_value);
    for item in &items[first_changed + 1..] {
        rebuilt.push(absolutize(item).unwrap_or_else(|| item.clone()));
    }
    Some(Arc::from(rebuilt.into_boxed_slice()))
}

/// Absolutize an optional slot position in place-style (None = unchanged).
fn absolutize_slot_opt(
    slot: &Option<TypeBodySlot>,
    canonical_id: &str,
) -> Option<Option<TypeBodySlot>> {
    slot.as_ref()
        .and_then(|slot| slot.absolutize(canonical_id))
        .map(Some)
}

impl NarrowTypeParam {
    fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        let constraint = absolutize_slot_opt(&self.constraint, canonical_id);
        let default = absolutize_slot_opt(&self.default, canonical_id);
        if constraint.is_none() && default.is_none() {
            return None;
        }
        Some(Self {
            name: self.name.clone(),
            ordinal: self.ordinal,
            constraint: constraint.unwrap_or_else(|| self.constraint.clone()),
            default: default.unwrap_or_else(|| self.default.clone()),
        })
    }
}

impl FunctionParamFact {
    fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        absolutize_slot_opt(&self.ty, canonical_id).map(|ty| Self { ty, ..self.clone() })
    }
}

impl FunctionSignatureFact {
    fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        let type_parameters =
            absolutize_fact_slice(&self.type_parameters, |p| p.absolutize(canonical_id));
        let parameters = absolutize_fact_slice(&self.parameters, |p| p.absolutize(canonical_id));
        let return_ty = absolutize_slot_opt(&self.return_ty, canonical_id);
        if type_parameters.is_none() && parameters.is_none() && return_ty.is_none() {
            return None;
        }
        Some(Self {
            type_parameters: type_parameters.unwrap_or_else(|| Arc::clone(&self.type_parameters)),
            parameters: parameters.unwrap_or_else(|| Arc::clone(&self.parameters)),
            return_ty: return_ty.unwrap_or_else(|| self.return_ty.clone()),
            has_implementation_body: self.has_implementation_body,
            spans_origin: self.spans_origin.clone(),
        })
    }
}

impl KeyTypeShape {
    fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        match self {
            KeyTypeShape::Other(slot) => slot.absolutize(canonical_id).map(KeyTypeShape::Other),
            KeyTypeShape::String | KeyTypeShape::Number | KeyTypeShape::Symbol => None,
        }
    }
}

impl ObjectMemberFact {
    fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        match self {
            ObjectMemberFact::Property(property) => {
                property.ty.absolutize(canonical_id).map(|ty| {
                    ObjectMemberFact::Property(ObjectPropertyFact {
                        ty,
                        ..property.clone()
                    })
                })
            }
            ObjectMemberFact::Method(method) => {
                method.function.absolutize(canonical_id).map(|function| {
                    ObjectMemberFact::Method(ObjectMethodFact {
                        function,
                        ..method.clone()
                    })
                })
            }
            ObjectMemberFact::CallSignature(signature) => signature
                .absolutize(canonical_id)
                .map(ObjectMemberFact::CallSignature),
            ObjectMemberFact::ConstructSignature(signature) => signature
                .absolutize(canonical_id)
                .map(ObjectMemberFact::ConstructSignature),
            ObjectMemberFact::IndexSignature(signature) => {
                let key_type = signature.key_type.absolutize(canonical_id);
                let value_type = signature.value_type.absolutize(canonical_id);
                if key_type.is_none() && value_type.is_none() {
                    return None;
                }
                Some(ObjectMemberFact::IndexSignature(IndexSignatureFact {
                    key_type: key_type.unwrap_or_else(|| signature.key_type.clone()),
                    value_type: value_type.unwrap_or_else(|| signature.value_type.clone()),
                    ..signature.clone()
                }))
            }
        }
    }
}

impl ObjectShapeFact {
    fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        absolutize_fact_slice(&self.members, |member| member.absolutize(canonical_id))
            .map(|members| Self { members })
    }
}

impl FactOrLocator {
    fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        match self {
            FactOrLocator::Leaf(_) | FactOrLocator::LeafUnion(_) | FactOrLocator::LeafObject(_) => {
                None
            }
            FactOrLocator::Locator(slot) => {
                slot.absolutize(canonical_id).map(FactOrLocator::Locator)
            }
            FactOrLocator::MacroPayload(payload) => payload
                .absolutize(canonical_id)
                .map(FactOrLocator::MacroPayload),
        }
    }
}

impl TuplePayloadFact {
    fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        absolutize_fact_slice(&self.elements, |element| {
            element
                .ty
                .absolutize(canonical_id)
                .map(|ty| TupleElementFact {
                    ty,
                    ..element.clone()
                })
        })
        .map(|elements| Self {
            readonly: self.readonly,
            elements,
        })
    }
}

impl IndexedAccessFact {
    fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        self.object.absolutize(canonical_id).map(|object| Self {
            object,
            index_path: Arc::clone(&self.index_path),
        })
    }
}

impl ResolvedLocalShape {
    fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        match self {
            ResolvedLocalShape::Object(members) => absolutize_fact_slice(members, |member| {
                member
                    .ty
                    .absolutize(canonical_id)
                    .map(|ty| SynthesizedMemberFact {
                        ty,
                        ..member.clone()
                    })
            })
            .map(ResolvedLocalShape::Object),
            ResolvedLocalShape::Tuple(tuple) => tuple
                .absolutize(canonical_id)
                .map(ResolvedLocalShape::Tuple),
            ResolvedLocalShape::IndexedAccess(access) => access
                .absolutize(canonical_id)
                .map(ResolvedLocalShape::IndexedAccess),
            ResolvedLocalShape::Leaf(_) => None,
            ResolvedLocalShape::Ref(symbol) => {
                symbol.absolutize(canonical_id).map(ResolvedLocalShape::Ref)
            }
        }
    }
}

impl ProjectedTypeFact {
    fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        match self {
            ProjectedTypeFact::Member(member) => member.ty.absolutize(canonical_id).map(|ty| {
                ProjectedTypeFact::Member(ProjectedMemberFact {
                    ty,
                    ..member.clone()
                })
            }),
            ProjectedTypeFact::IndexSignature(signature) => {
                let key_type = signature.key_type.absolutize(canonical_id);
                let value_type = signature.value_type.absolutize(canonical_id);
                if key_type.is_none() && value_type.is_none() {
                    return None;
                }
                Some(ProjectedTypeFact::IndexSignature(
                    ProjectedIndexSignatureFact {
                        key_type: key_type.unwrap_or_else(|| signature.key_type.clone()),
                        value_type: value_type.unwrap_or_else(|| signature.value_type.clone()),
                        ..signature.clone()
                    },
                ))
            }
            ProjectedTypeFact::CallSignature(signature) => signature
                .absolutize(canonical_id)
                .map(ProjectedTypeFact::CallSignature),
            ProjectedTypeFact::ConstructSignature(signature) => signature
                .absolutize(canonical_id)
                .map(ProjectedTypeFact::ConstructSignature),
            ProjectedTypeFact::MemberPath { base, path } => {
                base.absolutize(canonical_id)
                    .map(|base| ProjectedTypeFact::MemberPath {
                        base,
                        path: Arc::clone(path),
                    })
            }
            ProjectedTypeFact::CallableParams {
                base,
                signature_ordinal,
                first_param,
            } => base
                .absolutize(canonical_id)
                .map(|base| ProjectedTypeFact::CallableParams {
                    base,
                    signature_ordinal: *signature_ordinal,
                    first_param: *first_param,
                }),
            ProjectedTypeFact::IndexPosition {
                base,
                signature_ordinal,
                position,
            } => base
                .absolutize(canonical_id)
                .map(|base| ProjectedTypeFact::IndexPosition {
                    base,
                    signature_ordinal: *signature_ordinal,
                    position: *position,
                }),
            ProjectedTypeFact::Surface(surface) => {
                let members = absolutize_fact_slice(&surface.members, |member| {
                    member
                        .ty
                        .absolutize(canonical_id)
                        .map(|ty| ProjectedMemberFact {
                            ty,
                            ..member.clone()
                        })
                });
                let call_signatures =
                    absolutize_fact_slice(&surface.call_signatures, |s| s.absolutize(canonical_id));
                let construct_signatures =
                    absolutize_fact_slice(&surface.construct_signatures, |s| {
                        s.absolutize(canonical_id)
                    });
                let index_signatures =
                    absolutize_fact_slice(&surface.index_signatures, |signature| {
                        let key_type = signature.key_type.absolutize(canonical_id);
                        let value_type = signature.value_type.absolutize(canonical_id);
                        if key_type.is_none() && value_type.is_none() {
                            return None;
                        }
                        Some(ProjectedIndexSignatureFact {
                            key_type: key_type.unwrap_or_else(|| signature.key_type.clone()),
                            value_type: value_type.unwrap_or_else(|| signature.value_type.clone()),
                            ..signature.clone()
                        })
                    });
                if members.is_none()
                    && call_signatures.is_none()
                    && construct_signatures.is_none()
                    && index_signatures.is_none()
                {
                    return None;
                }
                Some(ProjectedTypeFact::Surface(ProjectedSurfaceFact {
                    members: members.unwrap_or_else(|| Arc::clone(&surface.members)),
                    call_signatures: call_signatures
                        .unwrap_or_else(|| Arc::clone(&surface.call_signatures)),
                    construct_signatures: construct_signatures
                        .unwrap_or_else(|| Arc::clone(&surface.construct_signatures)),
                    index_signatures: index_signatures
                        .unwrap_or_else(|| Arc::clone(&surface.index_signatures)),
                    has_index_signature: surface.has_index_signature,
                }))
            }
        }
    }
}

impl ClosedTypeFact {
    fn absolutize(&self, canonical_id: &str) -> Option<Self> {
        match self {
            ClosedTypeFact::Leaf(_) | ClosedTypeFact::LeafUnion(_) => None,
            ClosedTypeFact::Object(shape) => {
                shape.absolutize(canonical_id).map(ClosedTypeFact::Object)
            }
            ClosedTypeFact::Function(signature) => signature
                .absolutize(canonical_id)
                .map(ClosedTypeFact::Function),
            ClosedTypeFact::Tuple(tuple) => {
                tuple.absolutize(canonical_id).map(ClosedTypeFact::Tuple)
            }
            ClosedTypeFact::IndexedAccess(access) => access
                .absolutize(canonical_id)
                .map(ClosedTypeFact::IndexedAccess),
        }
    }
}

impl SemanticTypeSource {
    /// The source with every producer-local (empty-`canonical_id`) anchor —
    /// at ANY nesting depth — rewritten to the supplied owning canonical, so
    /// the value is SELF-ANCHORING when consumed under a different owner's
    /// scope (cross-owner fallthrough inheritance). Already-absolute anchors
    /// are never rewritten; the anchor-free closed leaf family and the
    /// synthetic slot-binding carrier (whose key carries its own scope) pass
    /// through untouched.
    pub fn absolutized_against(&self, canonical_id: &str) -> Self {
        let rewritten = match self {
            SemanticTypeSource::Authored(locator) => locator
                .absolutize(canonical_id)
                .map(SemanticTypeSource::Authored),
            SemanticTypeSource::Projected(fact) => fact
                .absolutize(canonical_id)
                .map(SemanticTypeSource::Projected),
            SemanticTypeSource::Synthesized(shape) => shape
                .absolutize(canonical_id)
                .map(SemanticTypeSource::Synthesized),
            SemanticTypeSource::Closed(fact) => fact
                .absolutize(canonical_id)
                .map(SemanticTypeSource::Closed),
            SemanticTypeSource::SyntheticSlotBinding(_) => None,
        };
        rewritten.unwrap_or_else(|| self.clone())
    }

    /// Whether this source's meaning still depends on the SCOPE it is raised
    /// under — it contains a bare `Ref` leaf spelling (resolved by name under
    /// the raise scope's name resolution) or a producer-local
    /// (empty-`canonical_id`) anchor at any nesting depth.
    ///
    /// Deep-walk companion of the absolutization family above. A source that
    /// is NOT scope-relative is fully anchored: every named position is
    /// pinned to an absolute canonical, so two EQUAL values name the same
    /// declaration regardless of which owner published them. Two equal
    /// scope-relative values from DIFFERENT origins may name DIFFERENT
    /// declarations (the same alias spelling in two children's files), so a
    /// cross-origin merge identity must fold the effective scope in.
    pub fn is_scope_relative(&self) -> bool {
        match self {
            SemanticTypeSource::Authored(locator) => authored_locator_scope_relative(locator),
            SemanticTypeSource::Projected(fact) => match fact {
                ProjectedTypeFact::Member(member) => slot_scope_relative(&member.ty),
                ProjectedTypeFact::IndexSignature(signature) => {
                    key_shape_scope_relative(&signature.key_type)
                        || slot_scope_relative(&signature.value_type)
                }
                ProjectedTypeFact::CallSignature(signature)
                | ProjectedTypeFact::ConstructSignature(signature) => {
                    function_fact_scope_relative(signature)
                }
                // The member-name path is scope-free; the base locator's
                // anchor decides (an empty producer-local anchor resolves
                // under the raise scope).
                ProjectedTypeFact::MemberPath { base, .. } => authored_locator_scope_relative(base),
                // The ordinal addressing is scope-free; the base locator's
                // anchor decides, exactly as for the member-path route.
                ProjectedTypeFact::CallableParams { base, .. }
                | ProjectedTypeFact::IndexPosition { base, .. } => {
                    authored_locator_scope_relative(base)
                }
                ProjectedTypeFact::Surface(surface) => {
                    surface.members.iter().any(|m| slot_scope_relative(&m.ty))
                        || surface
                            .call_signatures
                            .iter()
                            .chain(surface.construct_signatures.iter())
                            .any(function_fact_scope_relative)
                        || surface.index_signatures.iter().any(|s| {
                            key_shape_scope_relative(&s.key_type)
                                || slot_scope_relative(&s.value_type)
                        })
                }
            },
            SemanticTypeSource::Synthesized(shape) => synthesized_shape_scope_relative(shape),
            SemanticTypeSource::Closed(fact) => match fact {
                ClosedTypeFact::Leaf(leaf) => leaf_scope_relative(leaf),
                ClosedTypeFact::LeafUnion(leaves) => leaves.iter().any(leaf_scope_relative),
                ClosedTypeFact::Object(object) => {
                    object.members.iter().any(object_member_scope_relative)
                }
                ClosedTypeFact::Function(signature) => function_fact_scope_relative(signature),
                ClosedTypeFact::Tuple(tuple) => tuple
                    .elements
                    .iter()
                    .any(|e| fact_or_locator_scope_relative(&e.ty)),
                ClosedTypeFact::IndexedAccess(access) => slot_scope_relative(&access.object),
            },
            // The synthetic slot-binding carrier key carries its own scope.
            SemanticTypeSource::SyntheticSlotBinding(_) => false,
        }
    }
}

fn leaf_scope_relative(leaf: &LeafTypeFact) -> bool {
    matches!(leaf, LeafTypeFact::Ref(_))
}

fn anchor_scope_relative(anchor: &AuthoredAnchor) -> bool {
    anchor.canonical_id.is_empty()
}

fn slot_scope_relative(slot: &TypeBodySlot) -> bool {
    anchor_scope_relative(&slot.anchor)
}

fn slot_opt_scope_relative(slot: &Option<TypeBodySlot>) -> bool {
    slot.as_ref().is_some_and(slot_scope_relative)
}

fn authored_locator_scope_relative(locator: &AuthoredBodyLocator) -> bool {
    match locator {
        AuthoredBodyLocator::DeclBody(slot) => slot_scope_relative(slot),
        AuthoredBodyLocator::AugmentationBody(body) => anchor_scope_relative(&body.anchor),
        AuthoredBodyLocator::JsdocTypedefBody(body) => anchor_scope_relative(&body.anchor),
        AuthoredBodyLocator::MacroPayload(payload) => anchor_scope_relative(&payload.anchor),
    }
}

fn key_shape_scope_relative(key: &KeyTypeShape) -> bool {
    match key {
        KeyTypeShape::Other(slot) => slot_scope_relative(slot),
        KeyTypeShape::String | KeyTypeShape::Number | KeyTypeShape::Symbol => false,
    }
}

fn function_fact_scope_relative(signature: &FunctionSignatureFact) -> bool {
    signature.type_parameters.iter().any(|param| {
        slot_opt_scope_relative(&param.constraint) || slot_opt_scope_relative(&param.default)
    }) || signature
        .parameters
        .iter()
        .any(|param| slot_opt_scope_relative(&param.ty))
        || slot_opt_scope_relative(&signature.return_ty)
}

fn object_member_scope_relative(member: &ObjectMemberFact) -> bool {
    match member {
        ObjectMemberFact::Property(property) => slot_scope_relative(&property.ty),
        ObjectMemberFact::Method(method) => function_fact_scope_relative(&method.function),
        ObjectMemberFact::CallSignature(signature)
        | ObjectMemberFact::ConstructSignature(signature) => {
            function_fact_scope_relative(signature)
        }
        ObjectMemberFact::IndexSignature(signature) => {
            key_shape_scope_relative(&signature.key_type)
                || slot_scope_relative(&signature.value_type)
        }
    }
}

fn fact_or_locator_scope_relative(value: &FactOrLocator) -> bool {
    match value {
        FactOrLocator::Leaf(leaf) => leaf_scope_relative(leaf),
        FactOrLocator::LeafUnion(leaves) => leaves.iter().any(leaf_scope_relative),
        FactOrLocator::Locator(slot) => slot_scope_relative(slot),
        FactOrLocator::MacroPayload(payload) => anchor_scope_relative(&payload.anchor),
        FactOrLocator::LeafObject(members) => {
            members.iter().any(|member| leaf_scope_relative(&member.ty))
        }
    }
}

fn synthesized_shape_scope_relative(shape: &ResolvedLocalShape) -> bool {
    match shape {
        ResolvedLocalShape::Object(members) => members
            .iter()
            .any(|member| fact_or_locator_scope_relative(&member.ty)),
        ResolvedLocalShape::Tuple(tuple) => tuple
            .elements
            .iter()
            .any(|e| fact_or_locator_scope_relative(&e.ty)),
        ResolvedLocalShape::IndexedAccess(access) => slot_scope_relative(&access.object),
        ResolvedLocalShape::Leaf(leaf) => leaf_scope_relative(leaf),
        // A bare named-symbol reference resolves in the SYMBOL'S own
        // canonical scope when anchored; an empty anchor is the
        // producer-local convention (resolved under the raise scope).
        ResolvedLocalShape::Ref(symbol) => anchor_scope_relative(&symbol.anchor),
    }
}
