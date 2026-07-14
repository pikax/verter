//! Lazy declaration-body memo — the content-addressed body store one
//! `IndexedReady` artifact owns.
//!
//! The shallow declaration-header index ([`DeclHeaderIndex`]) is the
//! eager inventory; THIS memo materialises declaration BODIES on first
//! semantic demand, through the scheduler-side
//! [`DeclLoweringService`] retained snapshot (never a re-parse per
//! touch — native retains the snapshot on a worker thread it owns;
//! `wasm32` retains it in a single-thread thread-local shard, NOT a
//! service field, since the `Rc`-backed parse is `!Send`/`!Sync`),
//! and caches the owned results per symbol.
//! The memo is a FILE-ARTIFACT
//! child: its identity is the owning artifact's
//! `(canonical, whole_hash, parse_env_hash)` [`SnapshotKey`] — content-
//! addressed by construction, so a content edit produces a fresh memo
//! and the superseded one can never answer a new-content demand.
//! Overlay artifacts own their own memo instance; an overlay body can
//! therefore never populate a base read (and vice versa).
//!
//! Concurrency: one `OnceLock` per `(space, scope, name)` entry —
//! concurrent first-touch of one symbol lowers it ONCE; waiters block
//! cooperatively on the cell. The cell is cloned OUT of the map before
//! initialisation so no map shard lock is held across the lowering
//! call. A demanded statement that also declares sibling symbols
//! backfills exactly those siblings' entries (the work was actually
//! performed — path-independent population of only what the compute
//! produced).

use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};

use dashmap::DashMap;
use rustc_hash::{FxHashMap, FxHashSet};

use verter_parser::utils::oxc::script::raw_surface::{
    capture_statement_surfaces, merge_overload_groups, RawSourceSurface, SymbolSpace,
};
use verter_parser::utils::oxc::script::type_surface::{
    collect_statement_dependency_names, AnalyzedExternalTypeSource,
};
use verter_semantic::analysis::decl_headers::DeclHeaderIndex;
use verter_semantic::analysis::framework_facts::svelte::{
    lower_props_annotation_at, PropsAnnotationLowering,
};
use verter_semantic::analysis::type_eval::{
    AugmentationScopeKind, EnumMemberValue, EvalEnv, FunctionSignature, TypeDeclBody, TypeDeclKind,
    ValueDeclGroup, ValueDeclKind,
};
use verter_semantic::analysis::type_eval_build::{
    build_eval_env, lower_jsdoc_typedef_named, lower_statement_parts, register_statement_parts,
    BuildEvalEnvContext, LoweredSignatureParts, LoweredTypeDeclParts, LoweredValueDeclParts,
    StatementLowerCtx,
};
use verter_semantic::analysis::type_solver::prepared::{
    collect_heritage_base_facts, collect_key_domain_closedness_fact,
};
use verter_semantic::analysis::type_solver::{PreparedTypeDecl, ResolvedRootIdentity};
use verter_semantic::facts::{
    produce_shallow_route_facts, type_body_fingerprint, value_body_fingerprint, CrossDeclLens,
    EmptyRouteFactLens, HashOutcome, RouteFactLens, TransientTypeBody, UnresolvedLens,
    ValueBodyFingerprintInput,
};
use verter_type_expr::facts::{
    EnumMemberFact, EnumMemberNamesFact, EnumScalar, HeritageBaseFact, KeyDomainClosednessFact,
    NarrowTypeParam, ObjectShapeFact, PreparedMemberFact, PreparedProjectionClassFact,
    PreparedWrapperShapeFact, ShallowRouteFacts, ValueAnnotationClass, ValueTypeAnnotationFact,
};
use verter_type_expr::locators::{TypeBodyPathStep, TypeBodySlot};
use verter_type_expr::span_origins::DeclContributorAnchor;
use verter_type_expr::{ObjectExpr, TypeExpr, TypeParam};

use crate::decl_lowering::{DeclLoweringService, SnapshotKey, SnapshotLease};
use crate::fact_emission::{RouteLens, ShallowLens};
use crate::resolver_core::shallow_file_state::{collect_type_refs, collect_typeof_roots};
use crate::types::MetaProvenance;

pub(crate) mod locator_deref;
pub(crate) use locator_deref::{DerefedBodyShape, LocatorBodyDerefError};

/// The lazily lowered body of one TYPE declaration group (all same-name
/// contributors folded, exactly as the whole-env walk would fold them).
#[derive(Debug, Clone)]
pub struct LoweredTypeDecl {
    pub kind: TypeDeclKind,
    /// `TypeDeclBody::Single` or the `Merged` carrier — the same
    /// merge-aware body `TypeDeclGroup::merged_body` produces.
    pub body: TypeDeclBody,
    /// The decl-body content fingerprint — a memo-owned body FACT computed
    /// ONCE at lazy lowering time from the TRANSIENT lowered contributor
    /// bodies (enum groups: the value-derived projected scalar-union arms)
    /// through the shared [`type_body_fingerprint`] producer and the shared
    /// [`ShallowLens`]. Stored as the full [`HashOutcome`] so admission
    /// checks keep `budget_exceeded` / `visited_nodes`; readers
    /// ([`DeclBodyMemo::compat_type_body_hash_input`]) return this stored
    /// fact — no locator deref, no query-time re-lowering.
    pub body_hash: HashOutcome,
    /// Generic type parameters, unioned across contributors in source
    /// order.
    pub type_parameters: Vec<TypeParam>,
    /// Body reference names (the per-statement analyzer product), unioned
    /// across contributors.
    pub dep_names: FxHashSet<String>,
    /// Structural subset of [`dep_names`](Self::dep_names).
    pub structural_dep_names: FxHashSet<String>,
    /// The per-decl DIRECT route facts (whole-route edges / member edges /
    /// member-path seed edges / member names), produced graph-free at this
    /// lazy lowering from the same transient contributor bodies. The session
    /// route closures read these through the shared fact-closure core.
    pub route_facts: ShallowRouteFacts,
    /// `typeof` roots referenced by the merged lookup surface (sorted).
    pub typeof_root_names: Vec<String>,
    /// The NARROW type-parameter facts (name + ordinal + content-free bound
    /// locators), unioned first-seen-by-name across contributors in source
    /// order — the fact mirror of [`type_parameters`](Self::type_parameters)
    /// the prepared-decl builder copies (`PreparedTypeDecl.type_parameters`).
    pub narrow_type_parameters: Vec<NarrowTypeParam>,
    /// The prepared MEMBER-INDEX facts (name → header flags + content-free
    /// member-value locator + span-recovery origin), classified ONCE at this
    /// lazy lowering from the same transient contributor bodies through the
    /// shared `verter_semantic` prepared classifiers
    /// ([`PreparedTypeDecl::build_member_index`]) — a merged group's member
    /// locators carry their `MergedContributor` path step. The session
    /// prepared-decl builder COPIES these facts; it never re-classifies or
    /// derefs a locator at prepare time.
    pub member_index: FxHashMap<String, PreparedMemberFact>,
    /// The prepared structural-wrapper classification FACT, classified at
    /// this lazy lowering from the primary transient body
    /// ([`PreparedTypeDecl::classify_wrapper_shape`]).
    pub wrapper_shape: PreparedWrapperShapeFact,
    /// The prepared projection classification FACT, classified at this lazy
    /// lowering ([`PreparedTypeDecl::classify_projection`]).
    pub projection_class: PreparedProjectionClassFact,
    /// The producer-minted content-free heritage-base FACTS of a CLASS
    /// body's Intersection fold, extracted ONCE at this lazy lowering from
    /// the same transient contributor bodies through the shared
    /// `verter_semantic` extractor ([`collect_heritage_base_facts`]) — the
    /// authored base name + `name_resolution` routing key + per-argument
    /// [`verter_type_expr::locators::TypeArgLocator`]s. The session
    /// prepared-decl builder COPIES these facts
    /// (`PreparedTypeDecl.heritage_bases`); the dispatch resolves each head
    /// and lowers demanded arguments on demand — no query-time body re-walk.
    /// Empty for non-class declarations and heritage-free classes.
    pub heritage_bases: Arc<[HeritageBaseFact]>,
    /// The producer-minted per-declaration KEY-DOMAIN closedness fact
    /// (closed-object SHAPE verdict + one recipe per contributor body),
    /// extracted ONCE at this lazy lowering from the same transient
    /// contributor bodies through the shared `verter_semantic` extractor
    /// ([`collect_key_domain_closedness_fact`]). The session prepared-decl
    /// builder COPIES it (`PreparedTypeDecl.key_domain_closedness`); the
    /// dispatch closedness evaluator reads it in place of a query-time
    /// authored-body walk. `None` for enum groups (their type surface is the
    /// value-derived scalar union — no authored type body to classify).
    pub key_domain_closedness: Option<Arc<KeyDomainClosednessFact>>,
}

/// The memo-owned VALUE-body fingerprint FACT — the [`HashOutcome`] fields
/// carried NoTypeExpr-witnessed (the lower-crate outcome struct predates the
/// witness derive and cannot be annotated from the session). Lossless
/// bijection with [`HashOutcome`] via
/// [`from_outcome`](Self::from_outcome) / [`to_outcome`](Self::to_outcome).
#[derive(Debug, Clone, PartialEq, Eq, verter_no_typeexpr::NoTypeExpr)]
pub struct ValueBodyHashFact {
    /// The structural fingerprint.
    pub hash: verter_semantic::facts::FactHash,
    /// `true` when the producing fold could not fully observe the body — set
    /// by TWO DISTINCT mechanisms. (1) Depth-cap: the shared hash encoder
    /// (`enter_frame`, `verter_semantic` `facts/hashing.rs`) sets it at
    /// `MAX_HASH_DEPTH` exceedance for type AND value bodies alike (both
    /// walks share that encoder), including a real deep annotation on the
    /// demand-lowered file memo. (2) Transient-less fold: the shared session
    /// fold (`fold_lowered_value_decl` — reached via
    /// `lowered_value_decl_from_group` for a seeded/ambient VALUE fold, and
    /// via [`lowered_value_decl_for_synthesised_default`] for a synthesized
    /// component default) forces it on a record built without its
    /// fingerprint-relevant transients — VALUE-only; the type-side
    /// transient-less non-enum fold fails loudly in
    /// `lowered_type_decl_from_group` instead of setting the bit. The bit is
    /// stored honestly on the memo fact. At the PRE-EXISTING parse-domain
    /// body-fact admission ([`crate::fact_emission::LazyBodyFactSource`])
    /// the bit is dropped at `Fact` construction — that admission line is
    /// unchanged by this storage flip (for the depth-cap case the flow is
    /// byte-identical to the type side). TODO(follow-up): enforce
    /// `NonCacheable` on a `budget_exceeded` body fact at that shared
    /// admission.
    pub budget_exceeded: bool,
    /// Stable count of visited unique nodes (visit-order stability probes).
    pub visited_nodes: usize,
}

impl ValueBodyHashFact {
    fn from_outcome(outcome: HashOutcome) -> Self {
        Self {
            hash: outcome.hash,
            budget_exceeded: outcome.budget_exceeded,
            visited_nodes: outcome.visited_nodes,
        }
    }

    /// The [`HashOutcome`] view compat readers hand out.
    pub(crate) fn to_outcome(&self) -> HashOutcome {
        HashOutcome {
            hash: self.hash,
            budget_exceeded: self.budget_exceeded,
            visited_nodes: self.visited_nodes,
        }
    }
}

/// The lazily lowered body of one VALUE declaration group — narrowed FACTS
/// only, mirroring the fact vocabulary the inventory's
/// [`verter_semantic::analysis::type_eval::ValueDeclInfo`] carries, plus the
/// memo-owned value-body fingerprint. No `TypeExpr` is stored (compile-
/// witnessed by the `NoTypeExpr` derive): authored value positions are
/// content-free locators inside the facts, lowered on demand through the
/// shared dispatch.
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
pub struct LoweredValueDecl {
    pub kind: ValueDeclKind,
    /// The narrowed annotation FACT: classification
    /// ([`Absent`/`Direct`/`TypeOfAlias`](ValueAnnotationClass)), the
    /// precomputed single-hop `typeof x` peel target, and (when derivable)
    /// the annotation source.
    pub type_annotation: ValueTypeAnnotationFact,
    /// The merged overload signature-FACT set, in source order
    /// (`FunctionSignature` is the [`verter_type_expr::facts::FunctionSignatureFact`]
    /// alias; parameter/return positions are content-free body locators).
    pub signatures: Vec<FunctionSignature>,
    /// Narrowed object-shape fact, if this is a const initialized with an
    /// object (member value positions are content-free locators).
    pub object_shape: Option<ObjectShapeFact>,
    /// The full ordered narrowed member inventory of an `enum` declaration,
    /// in source declaration order, UNIONED across every same-name merged
    /// contributor ([`ValueDeclGroup::merged_enum_unified`]). `Some` exactly
    /// when the lowered value decl is an enum. Drives `typeof Enum` (an
    /// object keyed by the member NAMES) and the `Enum.Member` projection —
    /// EVERY member, foldable (literal scalar) or deferred-and-degraded
    /// (sound primitive domain). The value-body fingerprint reads the folded
    /// subset only.
    pub enum_members: Option<EnumMemberFact>,
    /// The enum's member-NAME inventory fact (the presence rail), mirrored
    /// from the inventory's producer-emitted
    /// [`ValueDeclInfo::enum_member_names`](verter_semantic::analysis::type_eval::ValueDeclInfo::enum_member_names)
    /// via [`ValueDeclGroup::merged_enum_member_names_fact`]. `Some` exactly
    /// when the lowered value decl is an enum.
    pub enum_member_names: Option<EnumMemberNamesFact>,
    /// The value-body content fingerprint — a memo-owned body FACT computed
    /// ONCE at lazy lowering time from the TRANSIENT lowered annotation /
    /// object shape (plus the merged signature facts and the folded enum
    /// members) through the shared [`value_body_fingerprint`] producer and
    /// the shared [`ShallowLens`] — the value-space sibling of
    /// [`LoweredTypeDecl::body_hash`]. Readers
    /// ([`crate::fact_emission::compat_value_body_hash_input`]) return this
    /// stored fact — no locator deref, no query-time re-lowering. A record
    /// built WITHOUT its lowering transients (a seeded env prefill or the
    /// ambient rune inventory) whose fingerprint would need them (a
    /// classified annotation or an object shape on a non-enum) carries a
    /// DEGRADED outcome (`budget_exceeded = true`) — an honest bit, never a
    /// fabricated fingerprint. Two distinct producer mechanisms set that
    /// bit (see [`ValueBodyHashFact::budget_exceeded`]): the transient-less
    /// DEGRADED bit is forced by the shared session fold
    /// (`fold_lowered_value_decl`, reached via
    /// `lowered_value_decl_from_group` and via the synthesized
    /// component-default constructor
    /// [`lowered_value_decl_for_synthesised_default`]), VALUE-only — the
    /// type-side transient-less non-enum fold fails loudly instead — while
    /// the shared hash encoder separately sets the same bit at
    /// `MAX_HASH_DEPTH` exceedance for real deep bodies, type and value
    /// alike. The
    /// parse-domain admission's bit-drop at `Fact` construction is
    /// pre-existing and unchanged here — the tracked NonCacheable-forcing
    /// follow-up at the shared admission owns it (see
    /// [`ValueBodyHashFact::budget_exceeded`]).
    pub body_hash: ValueBodyHashFact,
}

/// The committed value of one per-symbol demand cell.
///
/// The cell carries the [`LeaseMiss`](Self::LeaseMiss) outcome ITSELF (never a
/// thread-local side flag) so EVERY waiter that joins the initializer's
/// `get_or_init` observes the same outcome: a joiner can never read a
/// [`Ready(None)`](Self::Ready) the initializer meant as a transient no-warm
/// ReturnOnly. A `LeaseMiss` cell is EVICTED from its owning map (ptr-eq-guarded
/// so a fresh cell a concurrent retry installed is untouched) — a later demand
/// under a live lease recomputes; a `Ready(None)` is a genuine, cacheable
/// absence retained warm.
enum DemandCell<D> {
    Ready(Option<Arc<D>>),
    LeaseMiss,
}

type TypeCell = Arc<OnceLock<DemandCell<LoweredTypeDecl>>>;
type ValueCell = Arc<OnceLock<DemandCell<LoweredValueDecl>>>;
type LoweredDeclGroups = (
    Vec<(String, LoweredTypeDecl)>,
    Vec<(String, LoweredValueDecl)>,
);

/// Outcome of a demanded per-symbol lowering ([`DeclBodyMemo::lower_demanded`]).
///
/// The two `None`-shaped miss classes are DISTINCT and must be handled
/// differently by the caller's memo commit:
///
/// - [`Ready`](Self::Ready) — the lease-only run completed. `Some(batch)` is
///   the lowered product; `None` is a GENUINE miss (no service on a seeded
///   memo, or a fatal parse) whose body-less result is CORRECT and cacheable.
/// - [`LeaseMiss`](Self::LeaseMiss) — the lease pin was broken (unreachable in
///   practice): the lowering did not run and produced NOTHING. Fail CLOSED via
///   ReturnOnly — the caller must NOT memoize this as a body-less warm entry
///   (a silent wrong-empty result), in DEBUG *or* RELEASE. A later demand
///   under a live lease recovers.
enum DemandLower {
    Ready(Option<LoweredStatementBatch>),
    LeaseMiss,
}

/// Outcome of a per-symbol body DEMAND ([`DeclBodyMemo::demand_and_commit`])
/// as seen by a caller that needs to DISTINGUISH the two `None`-shaped miss
/// classes (the locator-deref path, which must not collapse a transient
/// ReturnOnly into a cacheable resolution result):
///
/// - [`Ready`](Self::Ready) — the lease-only run completed. `Some` is the
///   demanded decl; `None` is a GENUINE, cacheable miss (the symbol is not
///   inventoried, or the run produced a fatal-parse empty).
/// - [`LeaseMiss`](Self::LeaseMiss) — the lease pin was broken: the demand
///   ran NOTHING and committed NOTHING (`ReturnOnly`). A caller must route
///   this to a no-warm signal, never treat it as a genuine miss.
pub(crate) enum DemandOutcome<D> {
    Ready(Option<Arc<D>>),
    LeaseMiss,
}

impl<D> DemandOutcome<D> {
    /// Collapse to the plain `Option` API: a lease-miss reads as `None`. Used
    /// by the broad `Option`-returning demand accessors whose consumers do
    /// NOT distinguish the transient ReturnOnly from a genuine miss (the
    /// per-symbol demand cell already fails closed by evicting the poisoned
    /// cell, so a later demand under a live lease recovers).
    ///
    /// The `LeaseMiss` arm marks the generalized non-cacheability rail: this
    /// is the ONE central collapse point for the plain type / value /
    /// augmentation decl-body accessors, so a transient broken-lease read
    /// consumed by an enclosing traced compute refuses that compute's
    /// shared-cache admission (structural, not per-name). A `Ready(None)`
    /// genuine absence stays cacheable and marks nothing.
    fn into_option(self) -> Option<Arc<D>> {
        match self {
            DemandOutcome::Ready(value) => value,
            DemandOutcome::LeaseMiss => {
                crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                    crate::resolver_core::resolver_context::NonCacheableReadReason::LeaseMiss,
                );
                None
            }
        }
    }
}

/// TRANSIENT per-name retention between `lower_statement_parts` and
/// `register_statement_parts` inside one demanded lowering: the ordered
/// contributor bodies (fingerprint + classification input), their
/// contributor-statement anchors (span-origin minting), and the unioned
/// type-parameter headers. Fact-production intermediates — live only for the
/// duration of the lowering closure, never stored on the memo or any cache.
#[derive(Debug, Clone, Default)]
struct RetainedTypeTransients {
    /// Contributor bodies in source/binder order (the same order the
    /// registered group's contributors carry).
    bodies: Vec<TypeExpr>,
    /// Per-body contributor STATEMENT index (parallel to
    /// [`bodies`](Self::bodies)): `Some(program.body ordinal)` for a
    /// statement-lowered body — the [`DeclContributorAnchor`] the prepared
    /// member facts' span origins descend from — and `None` for a
    /// JSDoc-`@typedef` payload body (comment-derived, not
    /// statement-addressable: the honest `Synthetic` origin).
    contributor_indices: Vec<Option<u32>>,
    /// Type parameters unioned across contributors in source order,
    /// first-seen by name.
    type_parameters: Vec<TypeParam>,
}

impl RetainedTypeTransients {
    fn push(&mut self, parts: &LoweredTypeDeclParts, contributor_index: Option<u32>) {
        self.bodies.push(parts.body.clone());
        self.contributor_indices.push(contributor_index);
        for param in &parts.type_parameters {
            if !self.type_parameters.iter().any(|p| p.name == param.name) {
                self.type_parameters.push(param.clone());
            }
        }
    }

    fn extend_from(&mut self, other: RetainedTypeTransients) {
        self.bodies.extend(other.bodies);
        self.contributor_indices.extend(other.contributor_indices);
        for param in other.type_parameters {
            if !self.type_parameters.iter().any(|p| p.name == param.name) {
                self.type_parameters.push(param);
            }
        }
    }
}

/// TRANSIENT per-name VALUE-declaration retention inside one demanded
/// lowering — the value-space sibling of [`RetainedTypeTransients`]: the
/// last-wins contributor's lowered annotation / object shape, retained
/// between `lower_statement_parts` and `register_statement_parts` so the
/// value-body content fingerprint is computed AT LOWERING TIME from the same
/// lowering that registered the facts. Fact-production intermediates — live
/// only for the duration of the lowering closure, never stored on the memo
/// or any cache.
#[derive(Debug, Clone, Default)]
pub(crate) struct RetainedValueTransients {
    /// The LAST contributor's transient lowered annotation — strict
    /// last-wins, mirroring [`ValueDeclGroup::primary`] (the authoritative
    /// contributor), so the fingerprint observes the same annotation the
    /// legacy stored-field read observed.
    type_annotation: Option<TypeExpr>,
    /// The LAST contributor's transient lowered object shape (same
    /// last-wins rule).
    object_shape: Option<ObjectExpr>,
}

impl RetainedValueTransients {
    fn push(&mut self, parts: &LoweredValueDeclParts) {
        self.type_annotation = parts.type_annotation.clone();
        self.object_shape = parts.object_shape.clone();
    }
}

/// Owned product of one statement-batch lowering job: every symbol the
/// demanded statements actually declared, ready for entry population.
struct LoweredStatementBatch {
    types: Vec<(String, LoweredTypeDecl)>,
    values: Vec<(String, LoweredValueDecl)>,
    aug_types: Vec<(AugmentationScopeKind, String, LoweredTypeDecl)>,
    aug_values: Vec<(AugmentationScopeKind, String, LoweredValueDecl)>,
    /// Declaration-body contributors lowered by this job — the
    /// `decl_bodies_lowered` increment.
    lowered_count: usize,
}

/// See module docs.
pub struct DeclBodyMemo {
    key: SnapshotKey,
    eval_source: Arc<str>,
    framework_parse: Option<Arc<verter_language::FrameworkParseArtifact>>,
    source_type: oxc_span::SourceType,
    /// `None` on a seeded memo (every entry pre-filled; nothing to
    /// compute lazily).
    service: Option<Arc<DeclLoweringService>>,
    /// LEASE pinning this memo's retained parse snapshot for the lifetime
    /// of the memo (hence the owning `IndexedReady` artifact). Acquired
    /// lazily on the first service-backed body demand; dropped with the
    /// memo, releasing the retained parse. A seeded memo (no service)
    /// never holds a lease.
    lease: OnceLock<SnapshotLease>,
    header_index: Arc<DeclHeaderIndex>,
    provenance: Arc<MetaProvenance>,
    /// The ONE shared shallow cross-decl lens, built ONCE per state by
    /// [`ShallowLens::from_shallow`] and installed at the end of
    /// `ShallowFileState` construction (the lens derives from the FINISHED
    /// state, which owns this memo — so it cannot be a plain constructor
    /// argument). Consulted by the lowering-time body-fingerprint producer;
    /// the SAME `Arc` backs the lazy fact source, so there is exactly one
    /// lens instance per state.
    lens: OnceLock<Arc<ShallowLens>>,
    /// The ONE shared route-fact lens (hash-free full-import-target +
    /// header-membership view) the graph-free route producer classifies
    /// against — installed with the shallow lens at state construction.
    route_lens: OnceLock<Arc<RouteLens>>,
    type_entries: DashMap<String, TypeCell>,
    value_entries: DashMap<String, ValueCell>,
    aug_type_entries: DashMap<(AugmentationScopeKind, String), TypeCell>,
    aug_value_entries: DashMap<(AugmentationScopeKind, String), ValueCell>,
    whole_env: OnceLock<Arc<EvalEnv>>,
    raw_surfaces: DashMap<(String, SymbolSpace), Arc<Vec<RawSourceSurface>>>,
}

impl std::fmt::Debug for DeclBodyMemo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeclBodyMemo")
            .field("key", &self.key)
            .field("type_entries", &self.type_entries.len())
            .field("value_entries", &self.value_entries.len())
            .finish_non_exhaustive()
    }
}

impl DeclBodyMemo {
    /// Production constructor: an index-only memo whose bodies lower on
    /// first demand through `service`.
    ///
    /// `lease` carries the snapshot pin already acquired by the cold-index
    /// parse (the earliest service parse for this content generation) so
    /// the body demands reuse that one parse instead of re-parsing. When
    /// `None`, the memo acquires its own lease lazily on first body demand
    /// (see [`Self::ensure_lease`]).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        key: SnapshotKey,
        eval_source: Arc<str>,
        framework_parse: Option<Arc<verter_language::FrameworkParseArtifact>>,
        source_type: oxc_span::SourceType,
        service: Arc<DeclLoweringService>,
        header_index: Arc<DeclHeaderIndex>,
        provenance: Arc<MetaProvenance>,
        lease: Option<SnapshotLease>,
    ) -> Self {
        let lease_cell = OnceLock::new();
        if let Some(lease) = lease {
            let _ = lease_cell.set(lease);
        }
        Self {
            key,
            eval_source,
            framework_parse,
            source_type,
            service: Some(service),
            lease: lease_cell,
            header_index,
            provenance,
            lens: OnceLock::new(),
            route_lens: OnceLock::new(),
            type_entries: DashMap::default(),
            value_entries: DashMap::default(),
            aug_type_entries: DashMap::default(),
            aug_value_entries: DashMap::default(),
            whole_env: OnceLock::new(),
            raw_surfaces: DashMap::default(),
        }
    }

    /// Seeded constructor for the env-supplied construction path (test
    /// fixtures and other already-built-env callers): every entry is
    /// pre-filled from the built env using the same per-symbol folding
    /// the lazy path performs, and the whole env is pre-set. No service;
    /// nothing lowers lazily.
    pub(crate) fn seeded_from_env(
        key: SnapshotKey,
        env: &EvalEnv,
        analysis: &AnalyzedExternalTypeSource,
        header_index: Arc<DeclHeaderIndex>,
    ) -> Self {
        let memo = Self {
            key,
            eval_source: Arc::from(""),
            framework_parse: None,
            source_type: oxc_span::SourceType::ts(),
            service: None,
            lease: OnceLock::new(),
            header_index,
            provenance: Arc::new(MetaProvenance::default()),
            lens: OnceLock::new(),
            route_lens: OnceLock::new(),
            type_entries: DashMap::default(),
            value_entries: DashMap::default(),
            aug_type_entries: DashMap::default(),
            aug_value_entries: DashMap::default(),
            whole_env: OnceLock::new(),
            raw_surfaces: DashMap::default(),
        };

        for (name, group) in &env.type_symbols {
            let deps = analysis
                .local_type_symbol(name)
                .map(|symbol| {
                    (
                        symbol.dependency_names.clone(),
                        symbol.structural_dependency_names.clone(),
                    )
                })
                .unwrap_or_default();
            let enum_type_arms = env
                .value_symbols
                .get(name)
                .and_then(ValueDeclGroup::enum_type_union);
            // A seeded env carries locator-only groups: no transient lowered
            // bodies exist here, so only an ENUM group (whose fingerprint
            // derives from its scalar-union arms, which contain no `Ref`
            // sites — the lens is never consulted) can mint a `body_hash`
            // at seed time. A non-enum seeded group — single AND merged —
            // fails LOUDLY inside `lowered_type_decl_from_group`
            // (fail-lowering, never a fabricated fingerprint); seed callers
            // that need non-enum type cells must supply transient bodies.
            let lowered = lowered_type_decl_from_group(
                group,
                deps.0,
                deps.1,
                enum_type_arms,
                &RetainedTypeTransients::default(),
                &UnresolvedLens,
                &EmptyRouteFactLens,
            );
            memo.type_entries.insert(
                name.clone(),
                Arc::new(OnceLock::from(DemandCell::Ready(Some(Arc::new(lowered))))),
            );
        }
        for (name, group) in &env.value_symbols {
            // Seeded groups carry FACTS only — no transient lowered
            // annotation/shape exists here, so a non-enum record whose
            // fingerprint would need them carries the DEGRADED
            // `budget_exceeded` outcome, forced by the session fold
            // (`lowered_value_decl_from_group`) — a VALUE-only mechanism
            // (the seeded TYPE prefill above fails loudly instead), distinct
            // from the shared encoder's `MAX_HASH_DEPTH` depth-cap; an
            // honest bit with the pre-existing admission semantics
            // (see `ValueBodyHashFact::budget_exceeded`).
            let lowered = lowered_value_decl_from_group(group, None, &UnresolvedLens);
            memo.value_entries.insert(
                name.clone(),
                Arc::new(OnceLock::from(DemandCell::Ready(Some(Arc::new(lowered))))),
            );
        }
        for ((scope, name), group) in &env.augmentation_scopes {
            // Same seeded limitation as the file-scope type prefill above:
            // locator-only groups carry no transient bodies to fingerprint.
            let lowered = lowered_type_decl_from_group(
                group,
                FxHashSet::default(),
                FxHashSet::default(),
                None,
                &RetainedTypeTransients::default(),
                &UnresolvedLens,
                &EmptyRouteFactLens,
            );
            memo.aug_type_entries.insert(
                (scope.clone(), name.clone()),
                Arc::new(OnceLock::from(DemandCell::Ready(Some(Arc::new(lowered))))),
            );
        }
        for ((scope, name), group) in &env.augmentation_value_scopes {
            let lowered = lowered_value_decl_from_group(group, None, &UnresolvedLens);
            memo.aug_value_entries.insert(
                (scope.clone(), name.clone()),
                Arc::new(OnceLock::from(DemandCell::Ready(Some(Arc::new(lowered))))),
            );
        }
        let _ = memo.whole_env.set(Arc::new(env.clone()));
        memo
    }

    pub(crate) fn header_index(&self) -> &Arc<DeclHeaderIndex> {
        &self.header_index
    }

    /// Install the ONE shared shallow cross-decl lens. Called exactly once, at
    /// the end of `ShallowFileState` construction (the lens derives from the
    /// finished state), strictly before any body demand can reach
    /// [`Self::lower_demanded`]. Idempotent on a repeat set (first wins).
    pub(crate) fn install_shallow_lens(&self, lens: Arc<ShallowLens>) {
        let _ = self.lens.set(lens);
    }

    /// The shared shallow lens — the SAME `Arc` the lazy body-fact source
    /// carries, so the lowering-time fingerprint and the fact emission read
    /// one lens instance.
    pub(crate) fn shallow_lens(&self) -> Arc<ShallowLens> {
        Arc::clone(self.lens.get().expect(
            "ShallowLens is installed at ShallowFileState construction, before any body demand",
        ))
    }

    /// Install the ONE shared route-fact lens (the hash-free full-import-target
    /// view the graph-free route producer classifies against). Same lifecycle
    /// as [`Self::install_shallow_lens`]: installed exactly once at the end of
    /// `ShallowFileState` construction, strictly before any body demand;
    /// idempotent on a repeat set (first wins).
    pub(crate) fn install_route_fact_lens(&self, lens: Arc<RouteLens>) {
        let _ = self.route_lens.set(lens);
    }

    /// The shared route-fact lens.
    pub(crate) fn route_fact_lens(&self) -> Arc<RouteLens> {
        Arc::clone(self.route_lens.get().expect(
            "RouteLens is installed at ShallowFileState construction, before any body demand",
        ))
    }

    /// The canonical id this memo's snapshot lowers (anchors route-fact
    /// recipe locators).
    pub(crate) fn canonical_id(&self) -> Arc<str> {
        Arc::clone(&self.key.canonical)
    }

    /// The file's statically-classified [`FileLanguage`], derived from the
    /// memo's canonical id through the global registry (no host needed) so the
    /// lazy memo path stays self-contained. This is the rune-ambient
    /// classification source for both the whole-env oracle and the centralized
    /// effective-lookup.
    fn rune_module_file_language(&self) -> verter_language::FileLanguage {
        verter_language::LanguageRegistry::global()
            .classify_static(self.key.canonical.as_ref())
            .static_resolution()
    }

    /// Whether this file is a Svelte standalone rune module — the gate the
    /// centralized effective-lookup applies before consulting the rune
    /// ambient inventory (per-file scoping). Classified from the canonical id,
    /// so a plain `.ts` / `.js` never reports `true`.
    pub(crate) fn is_rune_module(&self) -> bool {
        crate::host_resolve::is_svelte_rune_module(&self.rune_module_file_language())
    }

    /// The retained framework parse artifact for this content generation, when
    /// the file is a framework carrier. This is the SAME artifact the indexing
    /// flight resolved — exposed so the component-default synth seam can read
    /// the carrier's module-script region without re-fetching it through
    /// `current_eval_state` (which re-enters `current_content_pinned_indexed`
    /// for the owner and recurses while the owner is mid-index).
    pub(crate) fn framework_parse(&self) -> Option<&Arc<verter_language::FrameworkParseArtifact>> {
        self.framework_parse.as_ref()
    }

    /// Acquire (once) the lease pinning this memo's retained parse
    /// snapshot for the rest of the memo's life. Called before every
    /// service-backed run so the snapshot stays warm across every body /
    /// whole-env / raw-surface demand on this content generation — a live
    /// artifact never silently re-parses. The single eval-program parse
    /// is counted HERE (the lease acquisition); every subsequent demand
    /// runs LEASE-ONLY (`run_leased`) against the pinned snapshot, so a
    /// broken pin is a lowering MISS, never a transient re-parse.
    /// A seeded memo (no service) never acquires a lease.
    fn ensure_lease(&self) {
        let Some(service) = self.service.as_ref() else {
            return;
        };
        self.lease.get_or_init(|| {
            let outcome = service.acquire_lease(&self.key, &self.eval_source, self.source_type);
            if outcome.parsed_now {
                self.provenance
                    .eval_program_parses
                    .fetch_add(1, Ordering::Relaxed);
            }
            outcome.lease
        });
    }

    /// Demand the lowered body of one file-scope TYPE symbol.
    pub(crate) fn type_decl(&self, name: &str) -> Option<Arc<LoweredTypeDecl>> {
        self.type_decl_outcome(name).into_option()
    }

    /// Demand the lowered body of one file-scope TYPE symbol, PRESERVING the
    /// lease-miss ReturnOnly outcome distinctly. The locator-deref path uses
    /// this so a broken-lease demand becomes a typed no-warm signal rather
    /// than collapsing into a cacheable genuine miss.
    pub(crate) fn type_decl_outcome(&self, name: &str) -> DemandOutcome<LoweredTypeDecl> {
        let Some((contributors, from_jsdoc)) = self
            .header_index
            .type_header(name)
            .map(|header| (header.contributors.clone(), header.from_jsdoc_typedef))
        else {
            // Not inventoried: a genuine, cacheable absence — never a
            // lease-miss.
            return DemandOutcome::Ready(None);
        };
        let cell = self
            .type_entries
            .entry(name.to_string())
            .or_default()
            .clone();
        // Backfill runs OUTSIDE the cell commit — see [`Self::backfill`]. The
        // initializing caller receives the batch and backfills siblings after
        // its own cell is committed; a lease-miss evicts the cell and commits
        // nothing.
        let (outcome, batch) = self.demand_and_commit(
            &cell,
            name,
            &contributors,
            from_jsdoc,
            |batch| {
                batch
                    .types
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, decl)| Arc::new(decl.clone()))
            },
            |poisoned| {
                self.type_entries
                    .remove_if(name, |_, existing| Arc::ptr_eq(existing, poisoned));
            },
        );
        if let Some(batch) = batch {
            self.backfill(batch, &contributors, Some((SymbolSpace::Type, name)), None);
        }
        outcome
    }

    /// Demand the lowered body of one file-scope VALUE symbol.
    pub(crate) fn value_decl(&self, name: &str) -> Option<Arc<LoweredValueDecl>> {
        self.value_decl_outcome(name).into_option()
    }

    /// Demand the lowered body of one file-scope VALUE symbol, PRESERVING the
    /// lease-miss ReturnOnly outcome distinctly (locator-deref no-warm rail).
    pub(crate) fn value_decl_outcome(&self, name: &str) -> DemandOutcome<LoweredValueDecl> {
        let Some(contributors) = self
            .header_index
            .value_header(name)
            .map(|header| header.contributors.clone())
        else {
            return DemandOutcome::Ready(None);
        };
        let cell = self
            .value_entries
            .entry(name.to_string())
            .or_default()
            .clone();
        let (outcome, batch) = self.demand_and_commit(
            &cell,
            name,
            &contributors,
            false,
            |batch| {
                batch
                    .values
                    .iter()
                    .find(|(n, _)| n == name)
                    .map(|(_, decl)| Arc::new(decl.clone()))
            },
            |poisoned| {
                self.value_entries
                    .remove_if(name, |_, existing| Arc::ptr_eq(existing, poisoned));
            },
        );
        if let Some(batch) = batch {
            self.backfill(batch, &contributors, Some((SymbolSpace::Value, name)), None);
        }
        outcome
    }

    /// The body fingerprint for a file-scope TYPE symbol — the single
    /// output/compat body-fact site on the memo side, used by the parse-time
    /// fact emitter to compute a body fingerprint (`semantic_hash` /
    /// `display_hash`) and nothing else.
    ///
    /// The fingerprint is computed ONCE, at lazy decl-body lowering time,
    /// from the transient lowered contributor bodies (see
    /// [`LoweredTypeDecl::body_hash`]); this accessor returns that stored
    /// memo-owned fact — no lens, no locator deref, no re-lowering. Demanding
    /// the symbol's body (the one lazy lowering) is the only work this read
    /// can trigger.
    pub(crate) fn compat_type_body_hash_input(&self, name: &str) -> Option<HashOutcome> {
        Some(self.type_decl(name)?.body_hash.clone())
    }

    /// Demand the lowered body of one augmentation-scoped TYPE symbol.
    pub(crate) fn augmentation_type_decl(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> Option<Arc<LoweredTypeDecl>> {
        self.augmentation_type_decl_outcome(scope, name)
            .into_option()
    }

    /// Demand the lowered body of one augmentation-scoped TYPE symbol,
    /// PRESERVING the lease-miss ReturnOnly outcome distinctly (locator-deref
    /// no-warm rail).
    pub(crate) fn augmentation_type_decl_outcome(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> DemandOutcome<LoweredTypeDecl> {
        let Some(contributors) = self
            .header_index
            .augmentation_type_header(scope, name)
            .map(|header| header.contributors.clone())
        else {
            return DemandOutcome::Ready(None);
        };
        let cell = self
            .aug_type_entries
            .entry((scope.clone(), name.to_string()))
            .or_default()
            .clone();
        let (outcome, batch) = self.demand_and_commit(
            &cell,
            name,
            &contributors,
            false,
            |batch| {
                batch
                    .aug_types
                    .iter()
                    .find(|(s, n, _)| s == scope && n == name)
                    .map(|(_, _, decl)| Arc::new(decl.clone()))
            },
            |poisoned| {
                self.aug_type_entries
                    .remove_if(&(scope.clone(), name.to_string()), |_, existing| {
                        Arc::ptr_eq(existing, poisoned)
                    });
            },
        );
        if let Some(batch) = batch {
            self.backfill(
                batch,
                &contributors,
                None,
                Some((scope, SymbolSpace::Type, name)),
            );
        }
        outcome
    }

    /// Demand the lowered body of one augmentation-scoped VALUE symbol.
    pub(crate) fn augmentation_value_decl(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> Option<Arc<LoweredValueDecl>> {
        self.augmentation_value_decl_outcome(scope, name)
            .into_option()
    }

    /// Demand the lowered body of one augmentation-scoped VALUE symbol,
    /// PRESERVING the lease-miss ReturnOnly outcome distinctly (locator-deref
    /// no-warm rail).
    fn augmentation_value_decl_outcome(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> DemandOutcome<LoweredValueDecl> {
        let Some(contributors) = self
            .header_index
            .augmentation_value_header(scope, name)
            .map(|header| header.contributors.clone())
        else {
            return DemandOutcome::Ready(None);
        };
        let cell = self
            .aug_value_entries
            .entry((scope.clone(), name.to_string()))
            .or_default()
            .clone();
        let (outcome, batch) = self.demand_and_commit(
            &cell,
            name,
            &contributors,
            false,
            |batch| {
                batch
                    .aug_values
                    .iter()
                    .find(|(s, n, _)| s == scope && n == name)
                    .map(|(_, _, decl)| Arc::new(decl.clone()))
            },
            |poisoned| {
                self.aug_value_entries
                    .remove_if(&(scope.clone(), name.to_string()), |_, existing| {
                        Arc::ptr_eq(existing, poisoned)
                    });
            },
        );
        if let Some(batch) = batch {
            self.backfill(
                batch,
                &contributors,
                None,
                Some((scope, SymbolSpace::Value, name)),
            );
        }
        outcome
    }

    /// The whole-file eval environment — a DEMAND product for whole-file
    /// consumers. Its most-hit consumer is `local_type_declaration_id`
    /// (type-decl identity resolution, reached on every `get_component_meta`
    /// resolution via `base_eval_env_arc`); the others are fallthrough,
    /// runtime values, and value-alias peeling. Built once through the
    /// retained snapshot and memoized; the per-symbol query path never
    /// touches it.
    pub fn whole_env(&self) -> Arc<EvalEnv> {
        // Warm path.
        if let Some(cached) = self.whole_env.get() {
            return cached.clone();
        }
        let Some(service) = self.service.as_ref() else {
            // Seeded memos pre-set the env; an un-seeded memo without a service
            // has no body to lower — the empty env is the CORRECT value, cache
            // it (this is a genuine miss, not a lease-pin break).
            return self
                .whole_env
                .get_or_init(|| Arc::new(EvalEnv::default()))
                .clone();
        };
        // Pin the retained snapshot for this memo's lifetime (parse counted at
        // lease acquisition); the LEASE-ONLY run below reuses it.
        self.ensure_lease();
        let build_ctx = BuildEvalEnvContext::new(Arc::clone(&self.key.canonical));
        let Some(mut env) = service.run_leased(&self.key, move |program| {
            program
                .map(|p| build_eval_env(p.borrow_dependent(), p.source_str(), &build_ctx))
                .unwrap_or_default()
        }) else {
            // Broken lease pin (unreachable in practice): fail CLOSED via
            // ReturnOnly. NEVER memoize the empty env — that is the silent
            // wrong-empty warm entry release builds used to admit; a retry
            // under a live lease recovers. Loud, not silent.
            tracing::error!(
                canonical = %self.key.canonical,
                "decl-body lease pin broken: whole_env's lease-only run missed the \
                 retained snapshot; failing closed to an uncached empty env (ReturnOnly)"
            );
            return Arc::new(EvalEnv::default());
        };
        self.provenance
            .eval_env_builds
            .fetch_add(1, Ordering::Relaxed);
        self.provenance
            .decl_bodies_lowered
            .fetch_add(env.total_decl_count() as u64, Ordering::Relaxed);
        // `<script setup generic="T">` parameters are NOT bound into this env:
        // they resolve through the dispatch `DeclarationScopePayload`
        // (`scope_type_bindings`, sourced from the prepared-decl bundle's
        // script-setup type bindings), consulted before any per-symbol
        // prepared-decl fallback — the same rail the per-symbol path uses.
        // A Svelte rune module (`.svelte.ts` / `.svelte.js`) merges the
        // module-valid runes into its whole env so its exported
        // rune-derived types infer correctly — per-file scoped, no
        // eval_source byte change. The runes are sourced from the SAME
        // centralized rune ambient inventory the graph-native
        // effective-lookup consults, so the oracle and the per-symbol
        // readers agree on rune visibility. Classify from the canonical
        // via the static registry (no host needed) so the lazy memo
        // path stays self-contained.
        let file_language = self.rune_module_file_language();
        crate::host_resolve::merge_rune_ambient_into_env(&mut env, &file_language);
        // Commit only the REAL env (idempotent — a cold race loses harmlessly).
        self.whole_env.get_or_init(|| Arc::new(env)).clone()
    }

    /// Whether the whole-file env has already been materialised (test
    /// observability — never a validity signal).
    #[cfg(test)]
    pub(crate) fn whole_env_materialized(&self) -> bool {
        self.whole_env.get().is_some()
    }

    /// Whether a per-symbol TYPE cell has a COMMITTED entry (test
    /// observability — never a validity signal). A lease-miss ReturnOnly
    /// leaves the (lazily-created) cell uninitialised, so this returns `false`.
    #[cfg(test)]
    pub(crate) fn type_entry_materialized(&self, name: &str) -> bool {
        self.type_entries
            .get(name)
            .is_some_and(|cell| matches!(cell.get(), Some(DemandCell::Ready(_))))
    }

    /// Whether a `(name, space)` raw-surface capture has a COMMITTED entry
    /// (test observability — never a validity signal). A lease-miss ReturnOnly
    /// never inserts, so this returns `false`.
    #[cfg(test)]
    pub(crate) fn raw_surfaces_materialized(&self, name: &str, space: SymbolSpace) -> bool {
        self.raw_surfaces.contains_key(&(name.to_string(), space))
    }

    /// Break the memo's worker-retained parse snapshot so the NEXT body
    /// demand lease-misses (test observability for the fail-closed ReturnOnly
    /// rail). The memo still HOLDS its `SnapshotLease` (so `ensure_lease`
    /// will not re-acquire), but the worker-side retained snapshot is
    /// released — mirroring the invariant-violation scenario. No-op on a
    /// seeded memo (no service).
    #[cfg(test)]
    pub(crate) fn release_retained_snapshot_for_test(&self) {
        if let Some(service) = self.service.as_ref() {
            service.release_retained_snapshot_for_test(&self.key);
        }
    }

    /// Demand the parse-time `RawSourceSurface` contributor vector for
    /// one `(name, symbol_space)` — captured from exactly the demanded
    /// symbol's contributing statements through the retained snapshot,
    /// memoized per triple.
    pub fn raw_surfaces_for(&self, name: &str, space: SymbolSpace) -> Arc<Vec<RawSourceSurface>> {
        if let Some(cached) = self.raw_surfaces.get(&(name.to_string(), space)) {
            return Arc::clone(&cached);
        }

        let mut contributors: Vec<u32> = Vec::new();
        match space {
            SymbolSpace::Type => {
                if let Some(header) = self.header_index.type_header(name) {
                    contributors.extend_from_slice(&header.contributors);
                }
                // An enum is registered dual-space, so its TYPE header above
                // already carries these locators; the dedicated enum table is
                // the member-NAME authority, and folding its contributor
                // locators in defensively keeps the capture complete even if a
                // refactor ever decoupled the two (deduped below).
                if let Some(header) = self.header_index.enum_headers.get(name) {
                    contributors.extend_from_slice(&header.contributors);
                }
            }
            SymbolSpace::Value => {
                if let Some(header) = self.header_index.value_header(name) {
                    contributors.extend_from_slice(&header.contributors);
                }
                if let Some(header) = self.header_index.enum_headers.get(name) {
                    contributors.extend_from_slice(&header.contributors);
                }
            }
        }
        contributors.sort_unstable();
        contributors.dedup();

        let surfaces =
            if let (false, Some(service)) = (contributors.is_empty(), self.service.as_ref()) {
                self.ensure_lease();
                let canonical = self.key.canonical.to_string();
                let wanted = name.to_string();
                // LEASE-ONLY run: never a transient re-parse. A broken lease
                // pin (the run misses the retained snapshot) fails CLOSED via
                // ReturnOnly BELOW — the empty capture is returned UNCACHED so
                // a lease-pin break can never silently memoize a wrong-empty
                // capture (in DEBUG *or* RELEASE).
                let leased = service.run_leased(&self.key, move |program| {
                    let Some(program) = program else {
                        return Vec::new();
                    };
                    let program = program.borrow_dependent();
                    let captured: Vec<_> = contributors
                        .iter()
                        .filter_map(|index| program.body.get(*index as usize))
                        .flat_map(capture_statement_surfaces)
                        .collect();
                    merge_overload_groups(captured)
                        .into_iter()
                        .filter(|c| c.name == wanted && c.symbol_space == space)
                        .map(|c| {
                            let mut surface = c.surface;
                            surface.decl_canonical = canonical.clone();
                            surface
                        })
                        .collect::<Vec<_>>()
                });
                let Some(surfaces) = leased else {
                    // Broken lease pin (unreachable in practice): ReturnOnly —
                    // return the empty capture WITHOUT memoizing it; a retry
                    // under a live lease recovers. Loud, not silent.
                    tracing::error!(
                        canonical = %self.key.canonical,
                        "decl-body lease pin broken: raw_surfaces_for's lease-only run \
                         missed the retained snapshot; failing closed to an uncached \
                         empty capture (ReturnOnly)"
                    );
                    return Arc::new(Vec::new());
                };
                surfaces
            } else {
                // No contributors / no service: a GENUINE empty capture — cache
                // it (the demanded symbol has no parse-time surfaces).
                Vec::new()
            };

        let surfaces = Arc::new(surfaces);
        self.raw_surfaces
            .insert((name.to_string(), space), Arc::clone(&surfaces));
        surfaces
    }

    /// Lower the demanded symbol's contributing statements through the
    /// retained snapshot, producing the owned per-symbol batch. `None`
    /// on a fatal parse — or on a broken lease pin (the LEASE-ONLY run
    /// fails CLOSED to the lowering miss, loudly in debug/test builds;
    /// it can never transiently re-parse).
    ///
    /// Like [`Self::whole_env`], this per-symbol path binds NO `<script
    /// setup generic="T">` parameter into its env: a script-setup generic
    /// is never resolved through a per-symbol `type_decl` demand. SFC
    /// own-file type bodies referencing `T` resolve through the dispatch
    /// `DeclarationScopePayload` (`scope_type_bindings`, sourced from the
    /// prepared-decl bundle's script-setup type bindings), which is
    /// consulted BEFORE any fallback to the per-symbol prepared-decl
    /// lookup — so the generic is already bound and never reaches this
    /// scratch env.
    fn lower_demanded(
        &self,
        name: &str,
        contributors: &[u32],
        from_jsdoc_typedef: bool,
    ) -> DemandLower {
        // A seeded memo has no service: nothing to lower, a genuine (cacheable)
        // body-less miss — NOT a lease-pin break.
        let Some(service) = self.service.as_ref() else {
            return DemandLower::Ready(None);
        };
        self.ensure_lease();
        let contributors = contributors.to_vec();
        let name = name.to_string();
        let build_ctx = BuildEvalEnvContext::new(Arc::clone(&self.key.canonical));
        let lens = self.shallow_lens();
        let route_lens = self.route_fact_lens();
        let outcome = service.run_leased(&self.key, move |program| {
            let program = program?;
            let source = program.source_str();
            let program = program.borrow_dependent();

            let mut scratch = EvalEnv::new();
            // TRANSIENT lowered TYPE bodies + type-parameter headers, retained
            // BETWEEN lowering and registration (the split
            // `lower_statement_parts` → `register_statement_parts` flow) so the
            // decl-body content fingerprint is computed HERE, at lowering time,
            // from the same lowering that registered the facts. Accumulated
            // across ALL demanded statements per declared name (contributor
            // source order) so a merged group fingerprints its FULL same-name
            // contributor set. Fact-production intermediates — dropped with
            // this closure, never stored.
            let mut retained_types: FxHashMap<String, RetainedTypeTransients> =
                FxHashMap::default();
            let mut retained_aug_types: FxHashMap<
                (AugmentationScopeKind, String),
                RetainedTypeTransients,
            > = FxHashMap::default();
            // TRANSIENT lowered VALUE annotations/shapes, retained the same
            // way for the value-body fingerprint (see
            // [`RetainedValueTransients`]).
            let mut retained_values: FxHashMap<String, RetainedValueTransients> =
                FxHashMap::default();
            let mut retained_aug_values: FxHashMap<
                (AugmentationScopeKind, String),
                RetainedValueTransients,
            > = FxHashMap::default();
            let mut dep_records: FxHashMap<String, (FxHashSet<String>, FxHashSet<String>)> =
                FxHashMap::default();
            for index in &contributors {
                let Some(stmt) = program.body.get(*index as usize) else {
                    continue;
                };
                let parts = lower_statement_parts(stmt, source);
                for decl in &parts.type_decls {
                    retained_types
                        .entry(decl.name.clone())
                        .or_default()
                        .push(decl, Some(*index));
                }
                for (scope, decl) in &parts.aug_type_decls {
                    retained_aug_types
                        .entry((scope.clone(), decl.name.clone()))
                        .or_default()
                        .push(decl, Some(*index));
                }
                for decl in &parts.value_decls {
                    retained_values
                        .entry(decl.name.clone())
                        .or_default()
                        .push(decl);
                }
                for (scope, decl) in &parts.aug_value_decls {
                    retained_aug_values
                        .entry((scope.clone(), decl.name.clone()))
                        .or_default()
                        .push(decl);
                }
                // `export default interface I` / `export default class C`
                // mirrors the declared-name type symbol under `default` at
                // registration; mirror the retained transients the same way so
                // the mirrored symbol fingerprints identically.
                if let Some(alias_from) = parts.alias_default_type_to.as_deref() {
                    if let Some(retained) = retained_types.get(alias_from).cloned() {
                        retained_types
                            .entry("default".to_string())
                            .or_default()
                            .extend_from(retained);
                    }
                }
                register_statement_parts(
                    parts,
                    StatementLowerCtx {
                        build: &build_ctx,
                        contributor_index: *index,
                    },
                    &mut scratch,
                );
                for (decl_name, deps) in collect_statement_dependency_names(stmt) {
                    let entry = dep_records.entry(decl_name).or_default();
                    entry.0.extend(deps.dependency_names);
                    entry.1.extend(deps.structural_dependency_names);
                }
            }
            if from_jsdoc_typedef {
                if let Some(typedef_body) = lower_jsdoc_typedef_named(
                    &program.comments,
                    source,
                    &name,
                    &build_ctx,
                    &mut scratch,
                ) {
                    // A JSDoc `@typedef` is NOT a statement, so the statement
                    // dep-collector never produces its reference edges. Derive
                    // the dependency roots from the RETAINED transient body of
                    // the same lowering that registered it, so the cached entry
                    // carries them (else the typedef caches with EMPTY deps →
                    // under-resolution + under-invalidation). Stored in BOTH the
                    // plain and structural sets: a typedef is an alias carrier,
                    // so its roots are structural for the required-import walk
                    // (conservative — never under-walks).
                    let mut refs = Vec::new();
                    collect_type_refs(&typedef_body, &mut refs);
                    let entry = dep_records.entry(name.clone()).or_default();
                    for reference in refs {
                        entry.0.insert(reference.clone());
                        entry.1.insert(reference);
                    }
                    let retained = retained_types.entry(name.clone()).or_default();
                    retained.bodies.push(typedef_body);
                    // A JSDoc `@typedef` payload is comment-derived — not
                    // statement-addressable, so its member span origins are
                    // the honest `Synthetic` miss.
                    retained.contributor_indices.push(None);
                }
            }

            let lowered_count = scratch.total_decl_count();
            let mut batch = LoweredStatementBatch {
                types: Vec::new(),
                values: Vec::new(),
                aug_types: Vec::new(),
                aug_values: Vec::new(),
                lowered_count,
            };
            let empty_retained = RetainedTypeTransients::default();
            for (decl_name, group) in &scratch.type_symbols {
                let (deps, structural) = dep_records.get(decl_name).cloned().unwrap_or_default();
                // An enum's type-space body is derived from its MERGED
                // value members (same name → matching value group), so the
                // type and value spaces never diverge.
                let enum_type_arms = scratch
                    .value_symbols
                    .get(decl_name)
                    .and_then(ValueDeclGroup::enum_type_union);
                let retained = retained_types.get(decl_name).unwrap_or(&empty_retained);
                batch.types.push((
                    decl_name.clone(),
                    lowered_type_decl_from_group(
                        group,
                        deps,
                        structural,
                        enum_type_arms,
                        retained,
                        lens.as_ref(),
                        route_lens.as_ref(),
                    ),
                ));
            }
            for (decl_name, group) in &scratch.value_symbols {
                batch.values.push((
                    decl_name.clone(),
                    lowered_value_decl_from_group(
                        group,
                        retained_values.get(decl_name),
                        lens.as_ref(),
                    ),
                ));
            }
            for ((scope, decl_name), group) in &scratch.augmentation_scopes {
                // Ambient augmentation blocks do not inventory enum
                // declarations, so no value-derived enum union applies here.
                let retained = retained_aug_types
                    .get(&(scope.clone(), decl_name.clone()))
                    .unwrap_or(&empty_retained);
                batch.aug_types.push((
                    scope.clone(),
                    decl_name.clone(),
                    lowered_type_decl_from_group(
                        group,
                        FxHashSet::default(),
                        FxHashSet::default(),
                        None,
                        retained,
                        lens.as_ref(),
                        route_lens.as_ref(),
                    ),
                ));
            }
            for ((scope, decl_name), group) in &scratch.augmentation_value_scopes {
                batch.aug_values.push((
                    scope.clone(),
                    decl_name.clone(),
                    lowered_value_decl_from_group(
                        group,
                        retained_aug_values.get(&(scope.clone(), decl_name.clone())),
                        lens.as_ref(),
                    ),
                ));
            }
            Some(batch)
        });
        // Outer `None` = a broken lease pin (the lease-only run missed the
        // retained snapshot; the job did NOT run). Fail CLOSED via ReturnOnly:
        // this must NEVER be memoized as a body-less warm entry, in DEBUG *or*
        // RELEASE (silent wrong-empty is the defect the prior debug-only
        // `debug_assert!` left latent in release). Loud, not silent
        // (fail-lowering, not silent-skip); a later demand under a live lease
        // recovers. Inner `Some/None` = the run completed (batch / fatal-parse
        // genuine miss) — the caller may cache it.
        let Some(inner) = outcome else {
            tracing::error!(
                canonical = %self.key.canonical,
                "decl-body lease pin broken: the demanded lowering's lease-only run \
                 missed the retained snapshot; failing closed to ReturnOnly (uncached)"
            );
            return DemandLower::LeaseMiss;
        };
        if let Some(batch) = inner.as_ref() {
            self.provenance
                .decl_bodies_lowered
                .fetch_add(batch.lowered_count as u64, Ordering::Relaxed);
        }
        DemandLower::Ready(inner)
    }

    /// Get-or-compute a per-symbol cell under `get_or_init` single-flight, with
    /// a lease-miss ReturnOnly rail.
    ///
    /// The demanded lowering runs INSIDE `get_or_init` so a symbol demanded
    /// concurrently lowers exactly ONCE (the hot-path single-flight contract).
    /// The committed cell is a [`DemandCell`]: a [`DemandLower::Ready`] commits
    /// [`DemandCell::Ready`] (with the extracted decl) and returns the batch so
    /// the initializing caller can backfill siblings; a
    /// [`DemandLower::LeaseMiss`] commits [`DemandCell::LeaseMiss`]. Because the
    /// no-warm signal is carried by the committed cell itself — never a
    /// thread-local side flag — EVERY waiter that joins the initializer's
    /// `get_or_init` reads the same `LeaseMiss` (a joiner can no longer observe
    /// the initializer's transient `None` as a false `Ready(None)`). Any
    /// observer of a `LeaseMiss` cell (initializer, joiner, or a re-demand of a
    /// not-yet-evicted poisoned cell) runs `on_lease_miss_evict`, which drops
    /// the poisoned cell from its owning map ptr-eq-guarded — so no future
    /// demand serves the wrong-empty warm entry and the next demand retries
    /// under a live lease. Fail CLOSED via ReturnOnly, in DEBUG *and* RELEASE.
    fn demand_and_commit<D>(
        &self,
        cell: &Arc<OnceLock<DemandCell<D>>>,
        name: &str,
        contributors: &[u32],
        from_jsdoc: bool,
        extract: impl FnOnce(&LoweredStatementBatch) -> Option<Arc<D>>,
        on_lease_miss_evict: impl FnOnce(&Arc<OnceLock<DemandCell<D>>>),
    ) -> (DemandOutcome<D>, Option<LoweredStatementBatch>) {
        // Warm / joiner-visible hit — the cell already carries a committed
        // outcome (this thread lost the init race or re-demands a warm cell).
        if let Some(committed) = cell.get() {
            return match committed {
                DemandCell::Ready(value) => (DemandOutcome::Ready(value.clone()), None),
                DemandCell::LeaseMiss => {
                    on_lease_miss_evict(cell);
                    (DemandOutcome::LeaseMiss, None)
                }
            };
        }
        let leftover: std::cell::Cell<Option<LoweredStatementBatch>> = std::cell::Cell::new(None);
        let committed =
            cell.get_or_init(
                || match self.lower_demanded(name, contributors, from_jsdoc) {
                    DemandLower::Ready(maybe_batch) => {
                        let decl = maybe_batch.as_ref().and_then(extract);
                        leftover.set(maybe_batch);
                        DemandCell::Ready(decl)
                    }
                    DemandLower::LeaseMiss => DemandCell::LeaseMiss,
                },
            );
        match committed {
            DemandCell::Ready(value) => (DemandOutcome::Ready(value.clone()), leftover.take()),
            // Surface the DISTINCT `LeaseMiss` outcome (so a caller that must
            // not collapse this transient ReturnOnly into a cacheable genuine
            // miss — the locator-deref path — routes it to a no-warm signal)
            // and evict the poisoned cell so the next demand retries.
            DemandCell::LeaseMiss => {
                on_lease_miss_evict(cell);
                (DemandOutcome::LeaseMiss, None)
            }
        }
    }

    /// Populate sibling entries the demanded statements ALSO declared
    /// (set-if-vacant; the demanded entry itself is excluded — it was
    /// already published by the `get_or_init` that produced this batch).
    ///
    /// Runs OUTSIDE the demanded cell's `get_or_init` closure, on the
    /// initializing thread only: publishing a sibling space with a
    /// blocking `OnceLock::set` while still holding the demanded cell's
    /// init-lock would deadlock against a concurrent demand of that
    /// sibling (a merged `class K {}` occupies BOTH the type and value
    /// space — type demand sets the value cell, value demand sets the type
    /// cell). With the demanded init-lock released first, a sibling `set`
    /// that races a concurrent initializer just returns `Err`; the
    /// concurrent initializer never waits on us, so no cycle forms.
    /// Coverage-gated: a sibling
    /// backfills ONLY when the lowered statement set covers ALL of that
    /// symbol's header contributors — a statement batch that lowered a
    /// SUBSET (the class half of an interface+class merge, demanded via
    /// its value side) must not pre-fill the full entry. Only recorded,
    /// actually lowered results enter — never broader pretend-coverage.
    fn backfill(
        &self,
        batch: LoweredStatementBatch,
        lowered_statements: &[u32],
        demanded_file_scope: Option<(SymbolSpace, &str)>,
        demanded_augmentation: Option<(&AugmentationScopeKind, SymbolSpace, &str)>,
    ) {
        let covers =
            |contributors: &[u32]| contributors.iter().all(|c| lowered_statements.contains(c));
        for (name, decl) in batch.types {
            if demanded_file_scope == Some((SymbolSpace::Type, name.as_str())) {
                continue;
            }
            if !self
                .header_index
                .type_header(&name)
                .is_some_and(|header| covers(&header.contributors))
            {
                continue;
            }
            let cell = self.type_entries.entry(name).or_default().clone();
            let _ = cell.set(DemandCell::Ready(Some(Arc::new(decl))));
        }
        for (name, decl) in batch.values {
            if demanded_file_scope == Some((SymbolSpace::Value, name.as_str())) {
                continue;
            }
            if !self
                .header_index
                .value_header(&name)
                .is_some_and(|header| covers(&header.contributors))
            {
                continue;
            }
            let cell = self.value_entries.entry(name).or_default().clone();
            let _ = cell.set(DemandCell::Ready(Some(Arc::new(decl))));
        }
        for (scope, name, decl) in batch.aug_types {
            if demanded_augmentation == Some((&scope, SymbolSpace::Type, name.as_str())) {
                continue;
            }
            if !self
                .header_index
                .augmentation_type_header(&scope, &name)
                .is_some_and(|header| covers(&header.contributors))
            {
                continue;
            }
            let cell = self
                .aug_type_entries
                .entry((scope, name))
                .or_default()
                .clone();
            let _ = cell.set(DemandCell::Ready(Some(Arc::new(decl))));
        }
        for (scope, name, decl) in batch.aug_values {
            if demanded_augmentation == Some((&scope, SymbolSpace::Value, name.as_str())) {
                continue;
            }
            if !self
                .header_index
                .augmentation_value_header(&scope, &name)
                .is_some_and(|header| covers(&header.contributors))
            {
                continue;
            }
            let cell = self
                .aug_value_entries
                .entry((scope, name))
                .or_default()
                .clone();
            let _ = cell.set(DemandCell::Ready(Some(Arc::new(decl))));
        }
    }
}

/// Owned TRANSIENT value-declaration parts of one demanded symbol, re-lowered
/// from the retained snapshot by [`DeclBodyMemo::transient_value_parts`] for
/// the locator-deref worker: the merged contributor view over the per-
/// statement [`LoweredValueDeclParts`] (last-wins annotation / object shape;
/// signatures concatenated in contributor order, so the vector index IS the
/// GROUP-level `ValueSignature` ordinal the producer-minted locators carry).
/// Fact-production intermediates — returned owned, never stored.
#[derive(Debug, Clone, Default)]
pub(crate) struct TransientValueParts {
    pub(crate) type_annotation: Option<TypeExpr>,
    pub(crate) object_shape: Option<ObjectExpr>,
    pub(crate) signatures: Vec<LoweredSignatureParts>,
    /// The owning declaration's HEADER type parameters — populated for a
    /// dual-space declaration (a `class K<T>` whose VALUE side's constructor
    /// shape references `T`) from the SAME statements' type-side parts,
    /// unioned first-seen-by-name. Empty for plain value declarations.
    pub(crate) type_parameters: Vec<TypeParam>,
}

impl DeclBodyMemo {
    /// TYPE-space transient contributor BODIES of one demanded file-scope
    /// symbol (source order; a JSDoc-`@typedef` name appends its re-derived
    /// payload body), re-lowered from the retained snapshot in a LEASE-ONLY
    /// job for the locator-deref worker. The demand cells are NOT touched and
    /// nothing is committed — the graph-tier `LowerLocator` memo owns caching
    /// the lowered product per `(locator, content)`; this service is its
    /// authored-body borrow. `Ready(None)` = not inventoried / no service /
    /// fatal parse (genuine, cacheable); `LeaseMiss` = broken lease pin
    /// (transient ReturnOnly).
    pub(crate) fn transient_type_bodies(&self, name: &str) -> DemandOutcome<Vec<TypeExpr>> {
        let Some((contributors, from_jsdoc)) = self
            .header_index
            .type_header(name)
            .map(|header| (header.contributors.clone(), header.from_jsdoc_typedef))
        else {
            return DemandOutcome::Ready(None);
        };
        self.transient_type_bodies_for(name, &contributors, from_jsdoc, None)
    }

    /// Augmentation-scoped sibling of [`Self::transient_type_bodies`].
    pub(crate) fn transient_augmentation_type_bodies(
        &self,
        scope: &AugmentationScopeKind,
        name: &str,
    ) -> DemandOutcome<Vec<TypeExpr>> {
        let Some(contributors) = self
            .header_index
            .augmentation_type_header(scope, name)
            .map(|header| header.contributors.clone())
        else {
            return DemandOutcome::Ready(None);
        };
        self.transient_type_bodies_for(name, &contributors, false, Some(scope))
    }

    /// Shared lease-only TYPE-body re-lowering over the demanded symbol's
    /// contributing statements. `aug_scope` selects the augmentation-scoped
    /// parts vector; `None` reads the file-scope parts (plus the
    /// `export default interface/class` mirror and the JSDoc-typedef payload).
    fn transient_type_bodies_for(
        &self,
        name: &str,
        contributors: &[u32],
        from_jsdoc: bool,
        aug_scope: Option<&AugmentationScopeKind>,
    ) -> DemandOutcome<Vec<TypeExpr>> {
        let Some(service) = self.service.as_ref() else {
            // Seeded memo: locator-only groups retain no authored source to
            // re-borrow — a genuine, cacheable body-less miss.
            return DemandOutcome::Ready(None);
        };
        self.ensure_lease();
        let contributors = contributors.to_vec();
        let name = name.to_string();
        let aug_scope = aug_scope.cloned();
        let build_ctx = BuildEvalEnvContext::new(Arc::clone(&self.key.canonical));
        let outcome = service.run_leased(&self.key, move |program| {
            let program = program?;
            let source = program.source_str();
            let program = program.borrow_dependent();
            let mut bodies: Vec<TypeExpr> = Vec::new();
            for index in &contributors {
                let Some(stmt) = program.body.get(*index as usize) else {
                    continue;
                };
                let parts = lower_statement_parts(stmt, source);
                match aug_scope.as_ref() {
                    Some(scope) => {
                        for (part_scope, decl) in &parts.aug_type_decls {
                            if part_scope == scope && decl.name == name {
                                bodies.push(decl.body.clone());
                            }
                        }
                    }
                    None => {
                        for decl in &parts.type_decls {
                            if decl.name == name {
                                bodies.push(decl.body.clone());
                            }
                        }
                        // `export default interface I` / `export default
                        // class C` mirrors the declared-name symbol under
                        // `default` — mirror the transient bodies the same
                        // way (see the demanded-lowering path).
                        if name == "default" {
                            if let Some(alias_from) = parts.alias_default_type_to.as_deref() {
                                for decl in &parts.type_decls {
                                    if decl.name == alias_from {
                                        bodies.push(decl.body.clone());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if from_jsdoc {
                let mut scratch = EvalEnv::new();
                if let Some(typedef_body) = lower_jsdoc_typedef_named(
                    &program.comments,
                    source,
                    &name,
                    &build_ctx,
                    &mut scratch,
                ) {
                    bodies.push(typedef_body);
                }
            }
            Some(bodies)
        });
        match outcome {
            // Broken lease pin: the job ran nothing — transient ReturnOnly.
            None => {
                tracing::error!(
                    canonical = %self.key.canonical,
                    "decl-body lease pin broken: transient type-body re-borrow \
                     missed the retained snapshot; failing closed to ReturnOnly"
                );
                DemandOutcome::LeaseMiss
            }
            // Fatal parse: a genuine, cacheable body-less miss.
            Some(None) => DemandOutcome::Ready(None),
            Some(Some(bodies)) => DemandOutcome::Ready(Some(Arc::new(bodies))),
        }
    }

    /// The re-derived JSDoc-`@typedef` payload body of one demanded typedef
    /// alias — the [`AuthoredBodyLocator::JsdocTypedefBody`] deref source.
    /// Lease-only; same outcome semantics as
    /// [`Self::transient_type_bodies`]. Serves ONLY the typedef payload
    /// (never a same-name TS declaration's statement body — the typedef
    /// locator addresses the comment-derived payload specifically).
    pub(crate) fn transient_jsdoc_typedef_body(&self, name: &str) -> DemandOutcome<TypeExpr> {
        let Some(service) = self.service.as_ref() else {
            return DemandOutcome::Ready(None);
        };
        self.ensure_lease();
        let name = name.to_string();
        let build_ctx = BuildEvalEnvContext::new(Arc::clone(&self.key.canonical));
        let outcome = service.run_leased(&self.key, move |program| {
            let program = program?;
            let source = program.source_str();
            let program = program.borrow_dependent();
            let mut scratch = EvalEnv::new();
            Some(lower_jsdoc_typedef_named(
                &program.comments,
                source,
                &name,
                &build_ctx,
                &mut scratch,
            ))
        });
        match outcome {
            None => {
                tracing::error!(
                    canonical = %self.key.canonical,
                    "decl-body lease pin broken: transient JSDoc-typedef re-borrow \
                     missed the retained snapshot; failing closed to ReturnOnly"
                );
                DemandOutcome::LeaseMiss
            }
            Some(None) => DemandOutcome::Ready(None),
            Some(Some(None)) => DemandOutcome::Ready(None),
            Some(Some(Some(body))) => DemandOutcome::Ready(Some(Arc::new(body))),
        }
    }

    /// The re-derived `$props()` binding-annotation payload at one demanded
    /// macro ordinal — the [`MacroPayloadPosition::TypeAnnotation`] deref
    /// source. Lease-only; same outcome semantics as
    /// [`Self::transient_type_bodies`].
    ///
    /// The position replays the capture's shared macro-ordinal walk
    /// ([`lower_props_annotation_at`]) over THIS memo's retained snapshot;
    /// the module-script region comes from the memo's OWN carrier artifact —
    /// every input is keyed by `self.key`, no read outside the lease. The
    /// yielded [`PropsAnnotationLowering`] keeps the two authored absences
    /// typed (`Unannotated` / `NoPropsCall`) so the deref maps each to its
    /// exact fail-closed error — never a fabricated body.
    ///
    /// [`MacroPayloadPosition::TypeAnnotation`]: verter_type_expr::locators::MacroPayloadPosition::TypeAnnotation
    pub(crate) fn transient_props_annotation_body(
        &self,
        macro_index: u32,
    ) -> DemandOutcome<PropsAnnotationLowering> {
        let Some(service) = self.service.as_ref() else {
            return DemandOutcome::Ready(None);
        };
        self.ensure_lease();
        let module_region = self
            .framework_parse
            .as_deref()
            .and_then(crate::parse::module_script_region);
        let outcome = service.run_leased(&self.key, move |program| {
            let program = program?;
            let source = program.source_str();
            let program = program.borrow_dependent();
            Some(lower_props_annotation_at(
                program,
                source,
                module_region,
                macro_index,
            ))
        });
        match outcome {
            None => {
                tracing::error!(
                    canonical = %self.key.canonical,
                    "decl-body lease pin broken: transient $props-annotation re-borrow \
                     missed the retained snapshot; failing closed to ReturnOnly"
                );
                DemandOutcome::LeaseMiss
            }
            Some(None) => DemandOutcome::Ready(None),
            Some(Some(lowering)) => DemandOutcome::Ready(Some(Arc::new(lowering))),
        }
    }

    /// The re-derived authored PER-FIELD macro payload at one demanded
    /// `(macro ordinal, field ordinal)` — the
    /// [`MacroPayloadPosition::Field`] deref source. Lease-only; same
    /// outcome semantics as [`Self::transient_type_bodies`].
    ///
    /// The position replays the analyzer's OWN macro assembly
    /// ([`verter_semantic::analysis::lower_macro_field_payload_at`] — one
    /// macro-ordinal / field-ordinal addressing engine, mint side and deref
    /// side cannot drift) over THIS memo's retained snapshot. The yielded
    /// [`MacroFieldPayloadLowering`] keeps the authored absences typed
    /// (`Unauthored` / `NoField`) so the deref maps each to its exact
    /// fail-closed error — never a fabricated body.
    ///
    /// [`MacroPayloadPosition::Field`]: verter_type_expr::locators::MacroPayloadPosition::Field
    /// [`MacroFieldPayloadLowering`]: verter_semantic::analysis::MacroFieldPayloadLowering
    pub(crate) fn transient_macro_field_payload(
        &self,
        macro_index: u32,
        field_index: u32,
    ) -> DemandOutcome<verter_semantic::analysis::MacroFieldPayloadLowering> {
        let Some(service) = self.service.as_ref() else {
            return DemandOutcome::Ready(None);
        };
        self.ensure_lease();
        let outcome = service.run_leased(&self.key, move |program| {
            let program = program?;
            let source = program.source_str();
            let program = program.borrow_dependent();
            Some(verter_semantic::analysis::lower_macro_field_payload_at(
                program,
                source,
                macro_index,
                field_index,
            ))
        });
        match outcome {
            None => {
                tracing::error!(
                    canonical = %self.key.canonical,
                    "decl-body lease pin broken: transient macro field-payload re-borrow \
                     missed the retained snapshot; failing closed to ReturnOnly"
                );
                DemandOutcome::LeaseMiss
            }
            Some(None) => DemandOutcome::Ready(None),
            Some(Some(lowering)) => DemandOutcome::Ready(Some(Arc::new(lowering))),
        }
    }

    /// Transient re-derivation of one macro call's authored generic TYPE
    /// ARGUMENT from THIS memo's retained snapshot — the lease-only
    /// hydration serving the macro hot mirror's sole producer
    /// (`macro_type_arg_hot_ref`). Replays the analyzer's own span address
    /// (`AnalyzedMacro.span`) through
    /// [`verter_semantic::analysis::lower_macro_type_argument_at_span`],
    /// so the mint side and the deref side share one address. `Ready(None)` =
    /// no macro-shaped call at that span / no authored type argument (a
    /// genuine typed absence); a broken lease pin is the DISTINCT
    /// `LeaseMiss` (transient ReturnOnly).
    pub(crate) fn transient_macro_type_argument(
        &self,
        macro_span: verter_span::Span,
    ) -> DemandOutcome<TypeExpr> {
        let Some(service) = self.service.as_ref() else {
            return DemandOutcome::Ready(None);
        };
        self.ensure_lease();
        let outcome = service.run_leased(&self.key, move |program| {
            let program = program?;
            let source = program.source_str();
            let program = program.borrow_dependent();
            Some(
                verter_semantic::analysis::lower_macro_type_argument_at_span(
                    program, source, macro_span,
                ),
            )
        });
        match outcome {
            // Service-level lease miss OR a program-absent re-borrow: both are
            // the transient broken-pin class — fail closed to ReturnOnly.
            None | Some(None) => {
                tracing::error!(
                    canonical = %self.key.canonical,
                    "decl-body lease pin broken: transient macro type-argument re-borrow \
                     missed the retained snapshot; failing closed to ReturnOnly"
                );
                DemandOutcome::LeaseMiss
            }
            // A genuine typed absence: no macro-shaped call at the span / no
            // authored type argument.
            Some(Some(None)) => DemandOutcome::Ready(None),
            Some(Some(Some(expr))) => DemandOutcome::Ready(Some(Arc::new(expr))),
        }
    }

    /// Recover one member's authored declaration-site spans from this memo's
    /// RETAINED parse via its producer-emitted span-recovery origin — the
    /// lease-pinned NO-PARSE wrapper over
    /// [`crate::locator_span_recovery::recover_member_spans`]. Any recovery
    /// failure (a seeded service-less memo, a broken lease pin, a stale
    /// origin) yields the DEFAULT (all-absent) spans — an honest absence,
    /// never a fabricated byte range.
    pub(crate) fn recover_member_spans_or_absent(
        &self,
        origin: &verter_type_expr::span_origins::MemberSpansOrigin,
    ) -> verter_type_expr::MemberSpans {
        let Some(service) = self.service.as_ref() else {
            return verter_type_expr::MemberSpans::default();
        };
        self.ensure_lease();
        crate::locator_span_recovery::recover_member_spans(service, &self.key, origin)
            .unwrap_or_default()
    }

    /// VALUE-space transient parts of one demanded file-scope symbol
    /// (last-wins annotation / object shape; GROUP-ordered signature IR),
    /// re-lowered from the retained snapshot in a LEASE-ONLY job for the
    /// locator-deref worker. Same outcome semantics as
    /// [`Self::transient_type_bodies`].
    pub(crate) fn transient_value_parts(&self, name: &str) -> DemandOutcome<TransientValueParts> {
        let Some(contributors) = self
            .header_index
            .value_header(name)
            .map(|header| header.contributors.clone())
        else {
            return DemandOutcome::Ready(None);
        };
        let Some(service) = self.service.as_ref() else {
            return DemandOutcome::Ready(None);
        };
        self.ensure_lease();
        let name = name.to_string();
        let outcome = service.run_leased(&self.key, move |program| {
            let program = program?;
            let source = program.source_str();
            let program = program.borrow_dependent();
            let mut merged = TransientValueParts::default();
            let mut found = false;
            for index in &contributors {
                let Some(stmt) = program.body.get(*index as usize) else {
                    continue;
                };
                let parts = lower_statement_parts(stmt, source);
                for decl in &parts.value_decls {
                    if decl.name != name {
                        continue;
                    }
                    found = true;
                    // Strict last-wins annotation/shape (the group's
                    // `primary()` rule); signatures CONCATENATE in
                    // contributor order — the index is the GROUP-level
                    // `ValueSignature` ordinal the minted locators carry.
                    merged.type_annotation = decl.type_annotation.clone();
                    merged.object_shape = decl.object_shape.clone();
                    merged.signatures.extend(decl.signatures.iter().cloned());
                }
                // A dual-space declaration's HEADER type parameters (a
                // `class K<T>` constructor shape references `T`) ride the
                // SAME statements' type-side parts — union them first-seen
                // by name so the deref binds the class's own binder shells.
                for decl in &parts.type_decls {
                    if decl.name != name {
                        continue;
                    }
                    for param in &decl.type_parameters {
                        if !merged
                            .type_parameters
                            .iter()
                            .any(|existing| existing.name == param.name)
                        {
                            merged.type_parameters.push(param.clone());
                        }
                    }
                }
            }
            Some(found.then_some(merged))
        });
        match outcome {
            None => {
                tracing::error!(
                    canonical = %self.key.canonical,
                    "decl-body lease pin broken: transient value-part re-borrow \
                     missed the retained snapshot; failing closed to ReturnOnly"
                );
                DemandOutcome::LeaseMiss
            }
            Some(None) => DemandOutcome::Ready(None),
            Some(Some(None)) => DemandOutcome::Ready(None),
            Some(Some(Some(parts))) => DemandOutcome::Ready(Some(Arc::new(parts))),
        }
    }
}

/// Fold EVERY type/value group of an already-built env into per-name lowered
/// records, with the fingerprint/classification TRANSIENTS re-lowered from
/// the given parsed program — the ambient-inventory construction path (the
/// rune prelude), sharing the ONE per-symbol fold the lazy memo uses so an
/// ambient record can never diverge from the memo-served shape. Uses the
/// lens-free [`UnresolvedLens`] / [`EmptyRouteFactLens`]: ambient records
/// never enter the parse fact rail, so cross-decl reference identity is
/// inert there.
pub(crate) fn lowered_decls_from_env_and_program(
    env: &EvalEnv,
    program: &oxc_ast::ast::Program<'_>,
    source: &str,
) -> LoweredDeclGroups {
    let mut retained_types: FxHashMap<String, RetainedTypeTransients> = FxHashMap::default();
    let mut retained_values: FxHashMap<String, RetainedValueTransients> = FxHashMap::default();
    for (index, stmt) in program.body.iter().enumerate() {
        let parts = lower_statement_parts(stmt, source);
        let contributor_index = Some(u32::try_from(index).unwrap_or(u32::MAX));
        for decl in &parts.type_decls {
            retained_types
                .entry(decl.name.clone())
                .or_default()
                .push(decl, contributor_index);
        }
        for decl in &parts.value_decls {
            retained_values
                .entry(decl.name.clone())
                .or_default()
                .push(decl);
        }
        if let Some(alias_from) = parts.alias_default_type_to.as_deref() {
            if let Some(retained) = retained_types.get(alias_from).cloned() {
                retained_types
                    .entry("default".to_string())
                    .or_default()
                    .extend_from(retained);
            }
        }
    }
    let empty_retained = RetainedTypeTransients::default();
    let types = env
        .type_symbols
        .iter()
        .map(|(name, group)| {
            let enum_type_arms = env
                .value_symbols
                .get(name)
                .and_then(ValueDeclGroup::enum_type_union);
            (
                name.clone(),
                lowered_type_decl_from_group(
                    group,
                    FxHashSet::default(),
                    FxHashSet::default(),
                    enum_type_arms,
                    retained_types.get(name).unwrap_or(&empty_retained),
                    &UnresolvedLens,
                    &EmptyRouteFactLens,
                ),
            )
        })
        .collect();
    let values = env
        .value_symbols
        .iter()
        .map(|(name, group)| {
            (
                name.clone(),
                lowered_value_decl_from_group(group, retained_values.get(name), &UnresolvedLens),
            )
        })
        .collect();
    (types, values)
}

/// Fold one same-name TYPE contributor group into the lazily-served
/// per-symbol record — the same body merge / parameter union the eager
/// shallow build performed per symbol.
///
/// `enum_type_arms` is the enum's value-derived scalar-union arm set, supplied
/// by the caller when this type name is an `enum` (see
/// [`ValueDeclGroup::enum_type_union`]). An enum's type-space body has NO
/// authored type-body position — the registered placeholder slot
/// (`merged_body()`) stays the LOCATOR carrier, while the arms drive the
/// `body_hash` fingerprint here and materialise as the actual union at the
/// graph layer on demand, derived from the MERGED value members so the type
/// and value spaces never diverge.
///
/// `retained` carries this lowering's TRANSIENT contributor bodies (source
/// order) + unioned type-parameter headers — the fingerprint / dep-derivation
/// inputs, read in place and dropped by the caller.
fn lowered_type_decl_from_group(
    group: &verter_semantic::analysis::type_eval::TypeDeclGroup,
    dep_names: FxHashSet<String>,
    structural_dep_names: FxHashSet<String>,
    enum_type_arms: Option<Vec<EnumScalar>>,
    retained: &RetainedTypeTransients,
    lens: &dyn CrossDeclLens,
    route_lens: &dyn RouteFactLens,
) -> LoweredTypeDecl {
    let primary = group.primary();
    // Dual-space enum knowledge rides the value-derived arms: a same-name
    // `enum` group merges (every contributor slot retained) even though its
    // type-space kind is the structural `Alias`.
    let body = group.merged_body_dual_space(enum_type_arms.is_some());
    let space = verter_semantic::facts::SymbolSpace::Type;

    // The fingerprint + member/typeof dep derivation read the SAME transient
    // view the legacy folded-body read observed: the enum scalar-union arms
    // for an enum group; every retained contributor body for a merged group;
    // the last (primary, last-wins) retained body for a single group.
    let (body_hash, dep_bodies): (HashOutcome, &[TypeExpr]) = match &enum_type_arms {
        Some(arms) => (
            // Scalar arms are literals / primitive domains — they carry no
            // `Ref` sites, no object members, and no `typeof` roots, so the
            // member/typeof derivations below see an empty body set.
            type_body_fingerprint(TransientTypeBody::EnumUnion(arms), space, lens),
            &[],
        ),
        None if body.is_merged() => {
            // Symmetric with the single-body branch below: a merged group with
            // NO retained transient contributor bodies (the seeded,
            // locator-only path) must fail loudly — hashing `Merged(&[])`
            // would mint a fabricated empty-object fingerprint for bodies
            // that were never lowered.
            assert!(
                !retained.bodies.is_empty(),
                "every registered merged type symbol retains the transient lowered contributor \
                 bodies of the lowerings that registered it (fail-lowering, never a fabricated \
                 fingerprint)",
            );
            (
                type_body_fingerprint(TransientTypeBody::Merged(&retained.bodies), space, lens),
                retained.bodies.as_slice(),
            )
        }
        None => {
            let primary_body = retained.bodies.last().expect(
                "every registered type symbol retains the transient lowered body of the \
                 lowering that registered it (fail-lowering, never a fabricated fingerprint)",
            );
            (
                type_body_fingerprint(TransientTypeBody::Single(primary_body), space, lens),
                std::slice::from_ref(primary_body),
            )
        }
    };

    // ONE graph-free route-fact production over the ordered contributor
    // list: duplicate member names across merged contributors fold with
    // FIRST-contributor precedence (the producer's property-level `seen` set
    // spans contributors, mirroring the legacy eager fold into one object
    // view) — matching the MergedDecl peer-merge property rule. The producer
    // walks ONLY these transient bodies: no sibling demand, no locator deref,
    // no cross-file resolution — same-file transitive closure lives in the
    // session route closures reading these stored facts.
    let route_facts = produce_shallow_route_facts(dep_bodies, route_lens);
    let mut typeof_roots = FxHashSet::default();
    for dep_body in dep_bodies {
        collect_typeof_roots(dep_body, &mut typeof_roots);
    }
    let mut typeof_root_names: Vec<String> = typeof_roots.into_iter().collect();
    typeof_root_names.sort_unstable();

    // NARROW type-parameter facts, unioned first-seen-by-name across
    // contributors in source order — the fact mirror of the retained
    // typed-IR parameter union above.
    let mut narrow_type_parameters: Vec<NarrowTypeParam> = Vec::new();
    for decl in group.contributors() {
        for param in decl.type_parameters.params.iter() {
            if !narrow_type_parameters
                .iter()
                .any(|existing| existing.name == param.name)
            {
                narrow_type_parameters.push(param.clone());
            }
        }
    }

    // Prepared classification FACTS (member index / wrapper shape /
    // projection class), classified at THIS lazy lowering from the SAME
    // transient contributor bodies the fingerprint observed, through the
    // shared verter_semantic prepared classifiers on a scratch
    // `PreparedTypeDecl` — the facts are copied off the scratch and the
    // transient bodies are dropped by the caller (the session prepared-decl
    // builder COPIES the stored facts; no re-classification, no locator
    // deref, no dispatch at prepare time). An enum type body has no authored
    // object body to classify (its type surface is the value-derived scalar
    // union), so it keeps the default facts.
    let anchor = &primary.body.anchor;
    let root_identity =
        ResolvedRootIdentity::new(anchor.canonical_id.as_ref(), anchor.symbol.as_ref());
    let mut scratch = PreparedTypeDecl::new(root_identity.clone(), primary.kind);
    scratch.type_parameters = narrow_type_parameters.clone();
    if enum_type_arms.is_none() {
        let contributor_anchor = |ordinal: usize| {
            retained
                .contributor_indices
                .get(ordinal)
                .copied()
                .flatten()
                .map(|contributor_index| DeclContributorAnchor { contributor_index })
        };
        if body.is_merged() {
            // Per-contributor member indexing so each member fact carries its
            // owning contributor's span-origin anchor AND its `MergedContributor`
            // locator step (the shared deref addresses a merged body's
            // sub-positions through a contributor step first). Duplicate names
            // fold with FIRST-contributor precedence — the MergedDecl peer-merge
            // property rule the legacy eager fold applied.
            let mut merged_index: FxHashMap<String, PreparedMemberFact> = FxHashMap::default();
            for (ordinal, contributor_body) in retained.bodies.iter().enumerate() {
                let mut per_contributor =
                    PreparedTypeDecl::new(root_identity.clone(), primary.kind);
                per_contributor.type_parameters = narrow_type_parameters.clone();
                per_contributor.build_member_index(contributor_body, contributor_anchor(ordinal));
                for (name, mut fact) in per_contributor.member_index {
                    if merged_index.contains_key(&name) {
                        continue;
                    }
                    let mut path: Vec<TypeBodyPathStep> =
                        Vec::with_capacity(fact.ty.path.len() + 1);
                    path.push(TypeBodyPathStep::MergedContributor {
                        ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
                    });
                    path.extend(fact.ty.path.iter().cloned());
                    fact.ty = TypeBodySlot {
                        anchor: fact.ty.anchor.clone(),
                        path: path.into(),
                    };
                    merged_index.insert(name, fact);
                }
            }
            scratch.member_index = merged_index;
            // Wrapper/projection classify over the primary (last-wins)
            // contributor body — merged interfaces are never mapped wrappers
            // or forward subjects, so this matches the legacy folded-view
            // classification per body shape.
            if let Some(primary_body) = retained.bodies.last() {
                scratch.classify_wrapper_shape(primary_body);
                scratch.classify_projection(primary_body);
            }
        } else if let Some(primary_body) = retained.bodies.last() {
            scratch.build_member_index(
                primary_body,
                contributor_anchor(retained.bodies.len().saturating_sub(1)),
            );
            scratch.classify_wrapper_shape(primary_body);
            scratch.classify_projection(primary_body);
        }
    }

    // Heritage-base FACTS of a CLASS body's Intersection fold, minted ONCE at
    // this lazy lowering from the SAME transient contributor bodies — a pure
    // syntactic extraction (no head resolution, no argument lowering; the
    // dispatch resolves heads and lowers demanded arguments). Gated on the
    // group's authoritative kind: only a CLASS Intersection fold encodes
    // heritage (an alias/interface intersection is not class heritage). A
    // merged group mints per contributor under its `MergedContributor` path
    // step so the argument locators deref through the merged body shape; a
    // single group mints from the primary (last-wins) body — the one body the
    // locator deref serves.
    let heritage_bases: Arc<[HeritageBaseFact]> =
        if enum_type_arms.is_none() && primary.kind == TypeDeclKind::Class {
            if body.is_merged() {
                let mut facts: Vec<HeritageBaseFact> = Vec::new();
                for (ordinal, contributor_body) in retained.bodies.iter().enumerate() {
                    let prefix = [TypeBodyPathStep::MergedContributor {
                        ordinal: u32::try_from(ordinal).unwrap_or(u32::MAX),
                    }];
                    facts.extend(collect_heritage_base_facts(
                        &root_identity,
                        contributor_body,
                        &prefix,
                    ));
                }
                facts.into()
            } else if let Some(primary_body) = retained.bodies.last() {
                collect_heritage_base_facts(&root_identity, primary_body, &[]).into()
            } else {
                Arc::from([])
            }
        } else {
            Arc::from([])
        };

    // KEY-DOMAIN closedness FACT, minted ONCE at this lazy lowering from the
    // SAME transient contributor bodies — a pure syntactic extraction
    // (binding-independent-sound arms only; everything else escapes by
    // locator to the dispatch-time node-route classifier). Enum groups mint
    // no fact: their type surface is the value-derived scalar union, and the
    // closedness evaluator reads the absent fact as UNAVAILABLE — matching
    // the body-less transient re-borrow the previous query-time walk hit.
    let key_domain_closedness: Option<Arc<KeyDomainClosednessFact>> = if enum_type_arms.is_none() {
        Some(Arc::new(collect_key_domain_closedness_fact(
            &root_identity,
            &retained.bodies,
            body.is_merged(),
        )))
    } else {
        None
    };

    LoweredTypeDecl {
        kind: primary.kind,
        body,
        body_hash,
        type_parameters: retained.type_parameters.clone(),
        dep_names,
        structural_dep_names,
        route_facts,
        typeof_root_names,
        narrow_type_parameters,
        member_index: scratch.member_index,
        wrapper_shape: scratch.wrapper_shape,
        projection_class: scratch.projection_class,
        heritage_bases,
        key_domain_closedness,
    }
}

/// Fold one same-name VALUE contributor group into the lazily-served
/// per-symbol record — a pure FACT COPY off the group (the facts were minted
/// by the inventory producer; nothing is re-derived from a raw body). An
/// enum's FULL member set is unioned across every same-name contributor via
/// [`ValueDeclGroup::merged_enum_unified`] (NOT `primary()`-only, which would
/// drop earlier merged declarations' members) so the type/value projection
/// surfaces and the value-body fingerprint both read from one lossless rail.
///
/// `retained` carries this lowering's TRANSIENT last-wins annotation / object
/// shape — the value-body fingerprint inputs, read in place and dropped by
/// the caller (the value-space mirror of the type-side `body_hash`
/// precedent). `None` (the seeded env prefill / ambient-inventory path) has
/// no transients: an enum still fingerprints fully from its folded member
/// facts, while a record whose fingerprint would need the missing transients
/// (a classified annotation, or an object shape on a non-enum) carries a
/// DEGRADED `budget_exceeded` outcome forced HERE, in this session fold — an
/// honest bit, never a fabricated fingerprint. That forcing is VALUE-only
/// (the type-side transient-less non-enum fold in
/// `lowered_type_decl_from_group` fails loudly instead of setting the bit)
/// and is distinct from the shared hash encoder's `MAX_HASH_DEPTH`
/// depth-cap, which sets the same bit for real deep bodies, type and value
/// alike. The parse-domain admission drops the bit at `Fact` construction —
/// pre-existing and unchanged by this storage flip (see
/// [`ValueBodyHashFact::budget_exceeded`]).
pub(crate) fn lowered_value_decl_from_group(
    group: &ValueDeclGroup,
    retained: Option<&RetainedValueTransients>,
    lens: &dyn CrossDeclLens,
) -> LoweredValueDecl {
    let primary = group.primary();
    fold_lowered_value_decl(
        primary.kind,
        primary.type_annotation.clone(),
        group.merged_signatures(),
        primary.object_shape.clone(),
        group.merged_enum_unified(),
        group.merged_enum_member_names_fact(),
        retained,
        lens,
    )
}

/// The single record-assembly + fingerprint core shared by EVERY
/// [`LoweredValueDecl`] producer — the group fold above and the synthesized
/// component-default constructor below — so the value-body fingerprint
/// convention and the honest degraded-bit rule have exactly one
/// implementation.
#[allow(clippy::too_many_arguments)]
fn fold_lowered_value_decl(
    kind: ValueDeclKind,
    type_annotation: verter_type_expr::facts::ValueTypeAnnotationFact,
    signatures: Vec<FunctionSignature>,
    object_shape: Option<verter_type_expr::facts::ObjectShapeFact>,
    enum_members: Option<verter_type_expr::facts::EnumMemberFact>,
    enum_member_names: Option<verter_type_expr::facts::EnumMemberNamesFact>,
    retained: Option<&RetainedValueTransients>,
    lens: &dyn CrossDeclLens,
) -> LoweredValueDecl {
    // Rail-explicit transient tuple view of the merged enum inventory — the
    // lossless [`EnumMemberValue::from_scalar`] bijection over the stored
    // scalars, minted for the fingerprint input and dropped (the fingerprint
    // reads the folded-literal subset only).
    let enum_tuples: Option<Vec<(String, EnumMemberValue)>> = enum_members.as_ref().map(|fact| {
        fact.members
            .iter()
            .map(|entry| {
                (
                    entry.name.clone(),
                    EnumMemberValue::from_scalar(&entry.value),
                )
            })
            .collect()
    });
    let (transient_annotation, transient_shape) = match retained {
        Some(retained) => (
            retained.type_annotation.as_ref(),
            retained.object_shape.as_ref(),
        ),
        None => (None, None),
    };
    // The enum arm of the fingerprint reads ONLY the folded member tuples
    // (fully fact-derived), so an enum never degrades. A transient-less
    // non-enum whose fingerprint would observe the annotation body or the
    // object shape (the two positions the facts no longer carry as typed IR)
    // degrades honestly instead of hashing an input the producer did not see.
    let is_enum = kind == ValueDeclKind::Enum && enum_tuples.is_some();
    let degraded = retained.is_none()
        && !is_enum
        && (!matches!(type_annotation.classification, ValueAnnotationClass::Absent)
            || object_shape.is_some());
    let mut body_hash = value_body_fingerprint(
        &ValueBodyFingerprintInput::new(
            transient_annotation,
            &signatures,
            kind,
            transient_shape,
            enum_tuples.as_deref(),
        ),
        verter_semantic::facts::SymbolSpace::Value,
        lens,
    );
    if degraded {
        body_hash.budget_exceeded = true;
    }
    LoweredValueDecl {
        kind,
        type_annotation,
        signatures,
        object_shape,
        enum_members,
        enum_member_names,
        body_hash: ValueBodyHashFact::from_outcome(body_hash),
    }
}

/// Build the synthesized COMPONENT-DEFAULT value record (`class default` with
/// one construct signature) from its fabricated public-instance SOURCE — the
/// framework synth legs' single [`LoweredValueDecl`] constructor
/// ([`crate::resolver_core::vue_default_synth`] /
/// [`crate::resolver_core::svelte_default_synth`]).
///
/// The instance shape rides the annotation FACT as the closed/synthesized
/// four-source arm ([`ValueAnnotationClass::Direct`] — the documented
/// classification for a type with no authored `TSType` node): authored member
/// payloads inside it stay content-free locators lowered on demand through
/// the one dispatch, never eagerly. The construct signature is honest to the
/// vocabulary: no parameters, no type parameters, and `return_ty: None` (the
/// return is the synthesized annotation source, not an authored position).
///
/// Fingerprinting routes through the SAME shared fold as the group producer:
/// a transient-less record with a classified annotation carries the honest
/// DEGRADED bit ([`ValueBodyHashFact::budget_exceeded`]) — the synthesized
/// source is not part of the fingerprint byte convention, and a fabricated
/// complete fingerprint would collide across distinct synthesized bodies.
/// Version identity rides the owner's `FileWholeHash` (the synth output is a
/// pure function of parse-domain inputs stored on content-addressed shallow
/// state). The lens is [`UnresolvedLens`] BY RULE: synthesis is a parse-domain
/// syntax-only producer forbidden from resolving imports (guard
/// `component_default_synth_parse_domain_only`), and the signature-bearing
/// fingerprint arm performs no reference resolution.
pub(crate) fn lowered_value_decl_for_synthesised_default(
    instance: verter_type_expr::facts::SemanticTypeSource,
) -> LoweredValueDecl {
    use verter_type_expr::span_origins::{FunctionSpansOrigin, SourceSynthetic};
    fold_lowered_value_decl(
        ValueDeclKind::Class,
        verter_type_expr::facts::ValueTypeAnnotationFact {
            typeof_alias_target: None,
            classification: ValueAnnotationClass::Direct,
            annotation: Some(instance),
        },
        vec![FunctionSignature {
            type_parameters: Arc::from(Vec::new().into_boxed_slice()),
            parameters: Arc::from(Vec::new().into_boxed_slice()),
            return_ty: None,
            has_implementation_body: true,
            spans_origin: FunctionSpansOrigin::Synthetic(SourceSynthetic),
        }],
        None,
        None,
        None,
        None,
        &UnresolvedLens,
    )
}

#[cfg(test)]
#[path = "decl_body_memo_tests.rs"]
mod decl_body_memo_tests;
