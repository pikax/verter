//! Dependency-neutral declaration-body lowering products. Every field type lives
//! in `verter_semantic::analysis::type_eval`, `verter_semantic::facts`, or
//! `verter_type_expr::facts` (this crate's own dependencies), and neither
//! type has an `impl` block reaching session/host state. `verter_session`
//! re-exports these values for its lowering machinery.
//!
//! The `DeclBodyMemo`/`DeclLoweringService` lazy lowering machinery stays
//! session-owned because it retains a
//! scheduler-side parse snapshot and blocks on a worker-thread rendezvous
//! (`DeclLoweringService::acquire_lease`) to lower on first demand, which
//! `verter_semantic`'s `ResolverObservation` (I/O-free, non-blocking) must
//! never do. This module owns only the lowered, content-free result values.

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::analysis::type_eval::{
    FunctionSignature, TypeDeclBody, TypeDeclInfo, TypeDeclKind, ValueDeclKind,
};
use crate::facts::{FactHash, HashOutcome};
use verter_type_expr::facts::{
    EnumMemberFact, EnumMemberNamesFact, FactPropertyKey, HeritageBaseFact,
    KeyDomainClosednessFact, NarrowTypeParam, ObjectShapeFact, PreparedMemberFact,
    PreparedProjectionClassFact, PreparedWrapperShapeFact, ShallowRouteFacts,
    TypeDependencyPathFact, ValueTypeAnnotationFact, VueIgnoredHeritageFact,
};

/// The lazily lowered body of one TYPE declaration group (all same-name
/// contributors folded, exactly as the whole-env walk would fold them). No
/// `TypeExpr` is stored (compile-witnessed by the `NoTypeExpr` derive):
/// authored contributor bodies and header-parameter BOUNDS are re-borrowed
/// lease-only from the retained snapshot on demand by `verter_session`'s
/// lowering machinery; this memo stores the content-free mirror facts
/// only.
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
pub struct LoweredTypeDecl {
    pub kind: TypeDeclKind,
    /// Content-free facts retained per exact source contributor. Member return
    /// inference is addressed through its producer-emitted origin only.
    pub contributor_facts: Arc<[TypeDeclInfo]>,
    /// `TypeDeclBody::Single` or the `Merged` carrier — the same
    /// merge-aware body `TypeDeclGroup::merged_body` produces.
    pub body: TypeDeclBody,
    /// The decl-body content fingerprint — a memo-owned body FACT computed
    /// ONCE at lazy lowering time from the TRANSIENT lowered contributor
    /// bodies (enum groups: the value-derived projected scalar-union arms)
    /// through the shared `type_body_fingerprint` producer and the shared
    /// `ShallowLens`. Stored as the full [`HashOutcome`] so admission
    /// checks keep `budget_exceeded` / `visited_nodes`; readers
    /// (`DeclBodyMemo::compat_type_body_hash_input`) return this stored
    /// fact — no locator deref, no query-time re-lowering.
    pub body_hash: HashOutcome,
    /// Semantic declaration-dependency segment identities. The root local
    /// binding and member path remain separate through classification.
    pub dependency_paths: FxHashSet<TypeDependencyPathFact>,
    pub structural_dependency_paths: FxHashSet<TypeDependencyPathFact>,
    /// Complete declaration carrier, including positions intentionally omitted
    /// from legacy component-meta closure breadth.
    pub declaration_carrier_paths: FxHashSet<TypeDependencyPathFact>,
    pub value_query_paths: FxHashSet<TypeDependencyPathFact>,
    pub value_position_paths: FxHashSet<TypeDependencyPathFact>,
    pub has_unroutable_value_position: bool,
    /// The per-decl DIRECT route facts (whole-route edges / member edges /
    /// member-path seed edges / member names), produced graph-free at this
    /// lazy lowering from the same transient contributor bodies. The session
    /// route closures read these through the shared fact-closure core.
    pub route_facts: ShallowRouteFacts,
    /// `typeof` roots referenced by the merged lookup surface (sorted).
    pub typeof_root_names: Vec<String>,
    /// The NARROW type-parameter facts (name + ordinal + content-free bound
    /// locators), unioned first-seen-by-name across contributors in source
    /// order — the content-free mirror of the transient typed-IR parameter
    /// union (the lease-only re-borrow serves bound CONTENT on demand). The
    /// prepared-decl builder copies this mirror
    /// (`PreparedTypeDecl.type_parameters`).
    pub narrow_type_parameters: Vec<NarrowTypeParam>,
    /// Exact typed `@vue-ignore` heritage identities copied from the shallow
    /// declaration header. Consumers apply them only under an explicit Vue
    /// runtime projection policy; ordinary inheritance remains unchanged.
    pub vue_ignored_heritage: Arc<[VueIgnoredHeritageFact]>,
    /// The prepared MEMBER-INDEX facts (name → header flags + content-free
    /// member-value locator + span-recovery origin), classified ONCE at this
    /// lazy lowering from the same transient contributor bodies through the
    /// shared `verter_semantic` prepared classifiers
    /// (`PreparedTypeDecl::build_member_index`) — a merged group's member
    /// locators carry their `MergedContributor` path step. The session
    /// prepared-decl builder COPIES these facts; it never re-classifies or
    /// derefs a locator at prepare time.
    pub member_index: FxHashMap<FactPropertyKey, PreparedMemberFact>,
    /// The prepared structural-wrapper classification FACT, classified at
    /// this lazy lowering from the primary transient body
    /// (`PreparedTypeDecl::classify_wrapper_shape`).
    pub wrapper_shape: PreparedWrapperShapeFact,
    /// The prepared projection classification FACT, classified at this lazy
    /// lowering (`PreparedTypeDecl::classify_projection`).
    pub projection_class: PreparedProjectionClassFact,
    /// The producer-minted content-free heritage-base FACTS of a CLASS
    /// body's Intersection fold, extracted ONCE at this lazy lowering from
    /// the same transient contributor bodies through the shared
    /// `verter_semantic` extractor (`collect_heritage_base_facts`) — the
    /// authored base name + `name_resolution` routing key + per-argument
    /// `verter_type_expr::locators::TypeArgLocator`s. The session
    /// prepared-decl builder COPIES these facts
    /// (`PreparedTypeDecl.heritage_bases`); the dispatch resolves each head
    /// and lowers demanded arguments on demand — no query-time body re-walk.
    /// Empty for non-class declarations and heritage-free classes.
    pub heritage_bases: Arc<[HeritageBaseFact]>,
    /// The producer-minted per-declaration KEY-DOMAIN closedness fact
    /// (closed-object SHAPE verdict + one recipe per contributor body),
    /// extracted ONCE at this lazy lowering from the same transient
    /// contributor bodies through the shared `verter_semantic` extractor
    /// (`collect_key_domain_closedness_fact`). The session prepared-decl
    /// builder COPIES it (`PreparedTypeDecl.key_domain_closedness`); the
    /// dispatch closedness evaluator reads it in place of a query-time
    /// authored-body walk. `None` for enum groups (their type surface is the
    /// value-derived scalar union — no authored type body to classify).
    pub key_domain_closedness: Option<Arc<KeyDomainClosednessFact>>,
}

/// The memo-owned VALUE-body fingerprint FACT — the [`HashOutcome`] fields
/// carried NoTypeExpr-witnessed. (`HashOutcome` now derives the witness
/// itself; this memo-local record stays the VALUE-side storage so the
/// record shape and its budget-bit doc convention do not move.) Lossless
/// bijection with [`HashOutcome`] via
/// [`from_outcome`](Self::from_outcome) / [`to_outcome`](Self::to_outcome).
#[derive(Debug, Clone, PartialEq, Eq, verter_no_typeexpr::NoTypeExpr)]
pub struct ValueBodyHashFact {
    /// The structural fingerprint.
    pub hash: FactHash,
    /// `true` when the producing fold could not fully observe the body — set
    /// by TWO DISTINCT mechanisms. (1) Depth-cap: the shared hash encoder
    /// (`enter_frame`, `verter_semantic` `facts/hashing.rs`) sets it at
    /// `MAX_HASH_DEPTH` exceedance for type AND value bodies alike (both
    /// walks share that encoder), including a real deep annotation on the
    /// demand-lowered file memo. (2) Transient-less fold: the shared session
    /// fold (`fold_lowered_value_decl` — reached via
    /// `lowered_value_decl_from_group` for a seeded/ambient VALUE fold, and
    /// via `lowered_value_decl_for_synthesised_default` for a synthesized
    /// component default) forces it on a record built without its
    /// fingerprint-relevant transients — VALUE-only; the type-side
    /// transient-less non-enum fold fails loudly in
    /// `lowered_type_decl_from_group` instead of setting the bit. The bit is
    /// stored honestly on the memo fact. At the PRE-EXISTING parse-domain
    /// body-fact admission (`crate::fact_emission::LazyBodyFactSource` on the
    /// `verter_session` side) the bit is dropped at `Fact` construction —
    /// that admission line is unchanged by this storage flip (for the
    /// depth-cap case the flow is byte-identical to the type side).
    /// Parse-domain admission currently does not consume this diagnostic bit.
    pub budget_exceeded: bool,
    /// Stable count of visited unique nodes (visit-order stability probes).
    pub visited_nodes: usize,
}

impl ValueBodyHashFact {
    /// Public constructor used by the session-owned lowering machinery.
    #[must_use]
    pub fn from_outcome(outcome: HashOutcome) -> Self {
        Self {
            hash: outcome.hash,
            budget_exceeded: outcome.budget_exceeded,
            visited_nodes: outcome.visited_nodes,
        }
    }

    /// The [`HashOutcome`] view consumed by session fact emission.
    #[must_use]
    pub fn to_outcome(&self) -> HashOutcome {
        HashOutcome {
            hash: self.hash,
            budget_exceeded: self.budget_exceeded,
            visited_nodes: self.visited_nodes,
        }
    }
}

/// The lazily lowered body of one VALUE declaration group — narrowed FACTS
/// only, mirroring the fact vocabulary the inventory's
/// `verter_semantic::analysis::type_eval::ValueDeclInfo` carries, plus the
/// memo-owned value-body fingerprint. No `TypeExpr` is stored (compile-
/// witnessed by the `NoTypeExpr` derive): authored value positions are
/// content-free locators inside the facts, lowered on demand through the
/// shared dispatch.
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
pub struct LoweredValueDecl {
    pub kind: ValueDeclKind,
    /// The narrowed annotation FACT: classification
    /// (`Absent`/`Direct`/`TypeOfAlias`), the precomputed single-hop
    /// `typeof x` peel target, and (when derivable) the annotation source.
    pub type_annotation: ValueTypeAnnotationFact,
    /// The merged overload signature-FACT set, in source order
    /// (`FunctionSignature` is the `verter_type_expr::facts::FunctionSignatureFact`
    /// alias; parameter/return positions are content-free body locators).
    pub signatures: Vec<FunctionSignature>,
    /// Narrowed object-shape fact, if this is a const initialized with an
    /// object (member value positions are content-free locators).
    pub object_shape: Option<ObjectShapeFact>,
    /// The full ordered narrowed member inventory of an `enum` declaration,
    /// in source declaration order, UNIONED across every same-name merged
    /// contributor (`ValueDeclGroup::merged_enum_unified`). `Some` exactly
    /// when the lowered value decl is an enum. Drives `typeof Enum` (an
    /// object keyed by the member NAMES) and the `Enum.Member` projection —
    /// EVERY member, foldable (literal scalar) or deferred-and-degraded
    /// (sound primitive domain). The value-body fingerprint reads the folded
    /// subset only.
    pub enum_members: Option<EnumMemberFact>,
    /// The enum's member-NAME inventory fact (the presence rail), mirrored
    /// from the inventory's producer-emitted
    /// `ValueDeclInfo::enum_member_names` via
    /// `ValueDeclGroup::merged_enum_member_names_fact`. `Some` exactly when
    /// the lowered value decl is an enum.
    pub enum_member_names: Option<EnumMemberNamesFact>,
    /// The value-body content fingerprint — a memo-owned body FACT computed
    /// ONCE at lazy lowering time from the TRANSIENT lowered annotation /
    /// object shape (plus the merged signature facts and the folded enum
    /// members) through the shared `value_body_fingerprint` producer and the
    /// shared `ShallowLens` — the value-space sibling of
    /// [`LoweredTypeDecl::body_hash`]. Readers
    /// (`crate::fact_emission::compat_value_body_hash_input` on the
    /// `verter_session` side) return this stored fact — no locator deref, no
    /// query-time re-lowering. A record built WITHOUT its lowering
    /// transients (a seeded env prefill or the ambient rune inventory) whose
    /// fingerprint would need them (a classified annotation or an object
    /// shape on a non-enum) carries a DEGRADED outcome
    /// (`budget_exceeded = true`) — an honest bit, never a fabricated
    /// fingerprint. Two distinct producer mechanisms set that bit (see
    /// [`ValueBodyHashFact::budget_exceeded`]): the transient-less DEGRADED
    /// bit is forced by the shared session fold (`fold_lowered_value_decl`,
    /// reached via `lowered_value_decl_from_group` and via the synthesized
    /// component-default constructor
    /// `lowered_value_decl_for_synthesised_default`), VALUE-only — the
    /// type-side transient-less non-enum fold fails loudly instead — while
    /// the shared hash encoder separately sets the same bit at
    /// `MAX_HASH_DEPTH` exceedance for real deep bodies, type and value
    /// alike. The parse-domain admission's bit-drop at `Fact` construction
    /// is independent of this stored diagnostic bit (see
    /// [`ValueBodyHashFact::budget_exceeded`]).
    pub body_hash: ValueBodyHashFact,
}
