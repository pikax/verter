//! Key-domain openness and closedness.
//!
//! The L1 carrier-stop predicates ([`utility_enumeration_domain_is_open_or_unknown`],
//! [`mapped_type_is_open_or_unknown`], [`mapped_type_key_domain_is_open_or_unknown`],
//! [`builtin_lowering_argument_is_open`]) and the fact-native closedness
//! proofs they delegate to ([`prepared_decl_body_is_closed`],
//! [`prepared_instantiation_key_domain_is_closed`]) are ONE evaluation: a
//! node walk over the typed IR that hops into declaration proofs, whose
//! recipes escape back into walks of lowered nodes. All of it runs on the
//! explicit work stack of [`Openness`], so the native stack does not grow
//! with the depth of an alias chain, a nest of mapped types or of
//! instantiation arguments, or a mix of them.
//!
//! A few leaves read through a dispatcher query instead — a conditional's
//! decision, a carrier head's resolution, an escape's lowering and the
//! one-hop reduction behind a builtin the registry rule cannot decide —
//! and that query may ask a gate here again, opening a nested evaluation
//! inside the step. A key-producing builtin over an argument the walk
//! judged open is decided open without that reduction, which could not
//! close it, so a nest of such builtins never re-enters the gate. Such a nested evaluation re-derives nothing
//! an evaluation of this transaction already proved: every declaration and
//! instantiation proof that completes is kept on the dispatch transaction
//! ([`ClosednessProofTable`]) and answers each later demand, and the query
//! the leaf issued is itself memoized by the dispatcher. A deep chain
//! therefore nests no deeper than the novelty of the questions it asks,
//! never one evaluation per hop.
//!
//! The semantic limits are deliberate and typed:
//!
//! - one work budget ([`ENUMERATION_DOMAIN_OPENNESS_NODE_BUDGET`] per
//!   predicate, or the caller's budget for a direct proof) is shared by
//!   every walk and proof one evaluation runs; its exhaustion is
//!   undecidable — OPEN for a key-domain question, CLOSED for the
//!   outer-generic question, `Unavailable` for a closedness proof — and the
//!   budget is deterministic, so the same inputs refuse the same way cold
//!   and warm: a reused proof charges the evaluation what it charged when
//!   it ran;
//! - a declaration already being proven on this dispatcher's chain is a
//!   refused cycle (`Unavailable`), never a proof;
//! - a proof is reused only where running it again would answer the same:
//!   one that met a context-dependent refusal (an exhausted budget, an
//!   unfinished or cancelled dispatcher read, a carrier being normalized
//!   below it, a non-cacheable read) is never kept, one whose cycle refusal
//!   names a proof below it is reused only while that proof is in flight
//!   ([`ClosednessChain`]), and a reuse replays the fact reads the proof
//!   rooted on the active fact tracer, so a warm entry built over it is
//!   invalidated by the same edits;
//! - inside a connected demand a proof records the cost receipt of the
//!   dispatcher reads it issued, and a reuse there first replays that
//!   receipt into the demand: the demand pays what running the proof
//!   would, and one that cannot pay — or a proof that ran in no demand —
//!   runs the proof and meets the refusal running it meets.

use std::rc::Rc;
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use verter_retention::heap_size::OwnedValue;
use verter_session_query::type_solver::prepared::PreparedTypeDecl;
use verter_type_expr::facts::ClosednessRecipe;
use verter_type_expr::locators::TypeBodySlot;
use verter_type_expr::{OwnedStr, TypeExpr};

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    DeclIdentity, ProjectionMode, ProjectionReductionContext, SemanticNodeData, SemanticNodeId,
};

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
pub fn userland_instantiation_body_is_closed_object<
    C: crate::resolver_core::ResolverCapabilities,
>(
    ctx: &dyn crate::resolver_core::ResolverContext<C>,
    base: &crate::semantic_query::DeclIdentity,
) -> bool {
    // The closed-object SHAPE verdict is a producer-minted FACT
    // (`KeyDomainClosednessFact.closed_object_shape`, extracted once at lazy
    // decl-body lowering from the transient contributor bodies) — no
    // query-time authored-body walk. An absent fact (seeded state, enum
    // group, no prepared decl) is not provably closed — the safe default.
    ctx.prepared_type_decl_return_only(
        base.canonical_id.as_ref(),
        base.owner,
        base.decl_name.as_ref(),
    )
    .and_then(|prepared| prepared.key_domain_closedness.clone())
    .is_some_and(|fact| fact.closed_object_shape)
}

/// Tri-state verdict of the fact-native KEY-DOMAIN closedness evaluator.
///
/// `Unavailable` is EXPLICIT and never collapses into `ProvenOpen`: an
/// unavailable fact (seeded state, broken lease, missing prepared decl,
/// unresolved name), a budget exhaustion, or an in-flight cycle refusal is a
/// REFUSAL to answer, not a proof of openness. Consumers that only need the
/// carrier-stop direction treat both non-`ProvenClosed` arms identically
/// (open-OR-UNKNOWN preserves the carrier — the L1 rule). No verdict outlives
/// the dispatch transaction, and within it a proof is reused only under the
/// conditions the module docs state, replaying the facts it rooted through
/// `observe_closedness_walk_consult`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosednessVerdict {
    /// The walk PROVED a finite enumerable key domain.
    ProvenClosed,
    /// The walk PROVED the key domain open (a free/open-bound type
    /// parameter in a key-reachable position, a non-enumerable surface, an
    /// arity-unsatisfiable instantiation).
    ProvenOpen,
    /// The walk could not decide: missing facts, unresolved names, budget
    /// exhaustion, cycle refusal, unlowerable positions. NEVER a proof.
    Unavailable,
}

impl ClosednessVerdict {
    pub(in crate::project_semantic_dispatch) fn is_closed(self) -> bool {
        matches!(self, ClosednessVerdict::ProvenClosed)
    }
}

/// Root one cross-file consult of the closedness walk on the active fact
/// tracer.
///
/// The openness/closedness predicates read OTHER canonical files than the
/// query's own inputs: transparent alias-chain hops, barrel re-export hops,
/// and prepared-decl bodies (`prepared_decl_body_is_closed` /
/// `prepared_instantiation_key_domain_is_closed`). The verdict — and
/// therefore the carrier-vs-materialise shape of the published value —
/// depends on every consulted file's content, so each consult must enter
/// the published entry's `ReadSetSignature.facts`: an edit to a chain file
/// then rejects the warm entry on the read-side validator and the verdict
/// recomputes (the read-side-authoritative cache rule). Emission rides the
/// SAME `observe_fan_out` rail the module-augmentation stitch uses
/// (`collect_augmentation_contributions`), so ANY enclosing
/// `install_fact_tracer` scope — the dispatch memo cold build, the
/// component-meta result compute, the materialise producer — picks the
/// fact up without route-specific plumbing; the rooting is
/// route/mode-independent like the carrier-stop itself.
///
/// The observed hash is the consult-time `IndexedReady.whole_hash` (one
/// atomic observation — never a separate current-content re-read). Non-file
/// canonicals (the builtin / synthetic / empty sentinels) and files unknown
/// to the live view observe nothing.
pub(super) fn observe_closedness_walk_consult<C: crate::resolver_core::ResolverCapabilities>(
    ctx: &dyn crate::resolver_core::ResolverContext<C>,
    canonical_id: &str,
) {
    if crate::semantic_query::is_non_file_base(canonical_id) {
        return;
    }
    // Structurally read-only: observes the consulted file's whole hash
    // onto the active tracer; fenced-ness flows via the chokepoint flag.
    if let Some(indexed) = ctx
        .ensure_indexed_ready_serve(canonical_id)
        .map(|serve| serve.indexed)
    {
        crate::resolver_core::resolver_context::observe_fan_out(
            verter_session_query::facts::fact_cache::FactVersionRef::FileWholeHash {
                canonical_id: canonical_id.to_string(),
                hash: indexed.whole_hash,
            },
        );
    }
}

/// Identity-preserving binding for ONE type-parameter slot of the
/// key-domain walks — the replacement for the bool-only `open_args`
/// environment. Beyond the open/closed verdict, a closed binding carries
/// the ACTUAL bound node identity where one is scope-safely available, so
/// the conditional branch-selection oracle can resolve a check/extends
/// operand that references a parameter bound to a concrete argument —
/// even while another (losing) branch contains an open parameter. All
/// identity is NODE identity (`SemanticNodeId`) — never a borrowed
/// `TypeExpr` (compile-witnessed by [`key_domain_binding_carries_no_type_expr`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyDomainBinding {
    /// Bound to an argument the walk judged OPEN.
    Open,
    /// Closed with no concrete identity: a mapper binder, an
    /// `infer`-introduced name, or an unfilled defaulted parameter pending
    /// default verification.
    ClosedAbstract,
    /// Closed, bound to an interned semantic node (the node route's
    /// argument identity — scope-free by construction; environment-free
    /// literal/primitive operands intern to their node identity at
    /// binding-normalisation time).
    ClosedNode(SemanticNodeId),
}

impl KeyDomainBinding {
    fn is_open(self) -> bool {
        matches!(self, KeyDomainBinding::Open)
    }
}

/// Compile-time witness that the live binding environment carries no
/// symbolic `TypeExpr` (node identity only) — the ClosedExpr borrow arm is
/// structurally unrepresentable.
#[allow(dead_code)]
fn key_domain_binding_carries_no_type_expr(binding: &KeyDomainBinding) {
    match binding {
        KeyDomainBinding::Open | KeyDomainBinding::ClosedAbstract => {}
        KeyDomainBinding::ClosedNode(id) => {
            let _: &SemanticNodeId = id;
        }
    }
}

/// The evaluator-layer binding environment of one declaration: one slot per
/// DECLARED type parameter, in declaration order. An EMPTY slot is a FREE
/// parameter (open); a filled slot is open or closed per
/// [`KeyDomainBinding`].
struct ParamBindings(Vec<Option<KeyDomainBinding>>);

impl ParamBindings {
    /// Every declared parameter free — a bare declaration reference binds
    /// nothing.
    fn free(prepared: &PreparedTypeDecl) -> Self {
        Self(vec![None; prepared.type_parameters.len()])
    }

    fn get(&self, prepared: &PreparedTypeDecl, name: &str) -> Option<KeyDomainBinding> {
        prepared
            .type_parameters
            .iter()
            .position(|param| param.name == name)
            .and_then(|slot| self.0[slot])
    }
}

/// One declaration under evaluation: its prepared facts and the live
/// binding of each declared type parameter, shared by every recipe frame of
/// that declaration.
struct RecipeEnv {
    prepared: OwnedValue<PreparedTypeDecl>,
    bindings: ParamBindings,
}

impl RecipeEnv {
    fn binding(&self, name: &str) -> Option<KeyDomainBinding> {
        self.bindings.get(&self.prepared, name)
    }
}

/// Operand-position policy axis of the node-level [`OpenWalk`].
///
/// The per-argument key-domain rule (an open argument confined to member
/// VALUE positions of a fixed-key body keeps the key domain CLOSED) is
/// sound only where the enclosing expression consumes the operand's KEY
/// SET. Where the enclosing operator consumes the operand's VALUES —
/// `Conditional.check` / `Conditional.extends` (branch selection relates
/// the operand's value structure) and `IndexedAccess.object` (the access
/// projects a member VALUE out of the object) — an instantiation is OPEN
/// if ANY argument is open. Conditional BRANCHES stay in the surrounding
/// position; `IndexedAccess.index` remains a key/keyspace question
/// (`KeyDomain`). The mapped `as`-clause name-remap policy is a ROLE
/// ([`OpenRole::MappedNameRemap`]), not a position — remap operands walk
/// pinned at `KeyDomain` under that role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum OperandPosition {
    /// A genuine KEY-DOMAIN position (`Pick`/`Omit` source, mapped
    /// source/keyspace, indexed-access index): instantiations are judged
    /// by the per-argument key-domain rule.
    KeyDomain,
    /// A VALUE-SENSITIVE operand (`Conditional.check`,
    /// `Conditional.extends`, `IndexedAccess.object`): an instantiation
    /// is OPEN if ANY argument is open.
    ValueSensitive,
}

/// Whether the prepared declaration `(canonical, name)` resolves to a
/// surface with a CLOSED enumerable key domain through a BOUNDED walk —
/// the fact-native Reader-C entry.
///
/// The verdict derives from the producer-minted per-declaration
/// [`verter_type_expr::facts::KeyDomainClosednessFact`] (one
/// [`verter_type_expr::facts::ClosednessRecipe`] per contributor body,
/// minted once at lazy decl-body lowering) evaluated by the recipe
/// evaluator ([`Openness::recipe`]) under an EMPTY binding environment (a
/// bare-decl reference binds nothing). Transparent alias hops follow the
/// prepared decl's `name_resolution` (NO route discovery); recipe ESCAPES
/// deref + shallow-lower the authored position through the ONE shared
/// lowerer and classify the NODE with the node-route walker — no query-time
/// `TypeExpr` decision anywhere on this path. Bounded by `budget` + the
/// dispatch-wide in-flight cycle guard, at any chain depth on a constant
/// native stack; one barrel re-export hop resolves a re-exported name.
///
/// `ProvenClosed` ⇒ the walk proves a finite key domain. `ProvenOpen` ⇒ a
/// proof of openness (a free type parameter in a key-reachable position, a
/// non-enumerable surface). `Unavailable` ⇒ a REFUSAL (missing prepared
/// decl / fact, unresolved name, genuine cycle, budget exhaustion) — never
/// collapsed into a proof; a refusal that depends on its context is never
/// reused.
pub fn prepared_decl_body_is_closed<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    canonical_id: &str,
    owner: verter_type_expr::TopLevelOwnerId,
    decl_name: &str,
    budget: &mut u32,
) -> ClosednessVerdict {
    let mut openness = Openness::new(dispatch, *budget);
    let verdict = openness
        .run(Ask::Decl {
            canonical_id: OwnedStr::from(canonical_id),
            owner,
            name: OwnedStr::from(decl_name),
        })
        .verdict();
    *budget = openness.budget;
    verdict
}

/// Whether the instantiation `base<args…>` produces a surface whose
/// ENUMERABLE KEY SET can be PROVEN to close, given per-argument
/// identity-preserving bindings — the fact-native Reader-D entry.
///
/// The instantiation's key domain closes only when:
///   1. the target declaration exists (a prepared decl is available, with
///      ONE barrel re-export hop on a miss),
///   2. its arity is satisfiable: `required_params <= arg_count <= total_params`
///      (the trailing `total_params - arg_count` params fall back to defaults),
///   3. every default supplied for an unfilled param is itself closed —
///      judged PATH-PRECISELY on the LOWERED default node (the authored
///      bound slot derefs + lowers under the accumulating environment, so a
///      default referencing an EARLIER parameter — `<T, U = T>` — forwards
///      that parameter's live identity), and a VERIFIED-CLOSED default
///      RE-BINDS its parameter to the lowered default NODE (the
///      branch-selection oracle then selects on concrete defaults), and
///   4. the prepared body's KEY SET is closed under those bindings — the
///      recipe evaluator applies the per-argument key-domain rule: an open
///      argument confined to member VALUE positions of a fixed-key body
///      does NOT open the key domain (the node walk never descends value
///      surfaces under the `KeyDomainProof` role), so `Omit<Foo<T>, …>`
///      still enumerates path-precisely — WITHOUT materialising members.
///
/// Bounded by the same shared budget; arity mismatches are PROVEN open
/// (decided from available facts); missing facts/defaults are UNAVAILABLE.
pub fn prepared_instantiation_key_domain_is_closed<
    C: crate::resolver_core::ResolverCapabilities,
>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    base: &DeclIdentity,
    args: &[KeyDomainBinding],
    budget: &mut u32,
) -> ClosednessVerdict {
    let mut openness = Openness::new(dispatch, *budget);
    let verdict = openness
        .run(Ask::Instantiation {
            base: base.clone(),
            args: args.to_vec(),
        })
        .verdict();
    *budget = openness.budget;
    verdict
}

/// One authored body position (a recipe escape slot OR a `T extends C` /
/// `T = D` bound slot), derefed LEASE-ONLY through the anchor canonical's
/// retained snapshot via the shared locator-deref worker. `None` =
/// unavailable — conservative (undecidable ⇒ refusal).
pub(in crate::project_semantic_dispatch) fn deref_slot_body<
    C: crate::resolver_core::ResolverCapabilities,
>(
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext<C>,
    slot: &verter_type_expr::locators::TypeBodySlot,
) -> Option<TypeExpr> {
    let serve = ctx.ensure_indexed_ready_serve(slot.anchor.canonical_id.as_ref())?;
    match ctx.deref_authored_body(
        &serve.indexed.shallow_state,
        &verter_type_expr::locators::AuthoredBodyLocator::DeclBody(slot.clone()),
    ) {
        Ok(verter_session_query::source::deref::DerefedAuthoredBody {
            shape: verter_session_query::source::deref::DerefedBodyShape::Single(expr),
            ..
        }) => Some(expr),
        // ONLY a broken decl-body lease pin marks non-cacheability: a
        // transient `LeaseMiss` collapsed to `None` here (undecidable ⇒
        // refusal) must taint the enclosing traced compute's admission so a
        // later live-lease demand recomputes. Every other error variant
        // (`UnknownSymbol`, canonical mismatch, path/annotation absence) is a
        // DETERMINISTIC refusal — cacheable, never marked.
        Err(verter_session_query::source::deref::LocatorBodyDerefError::LeaseMiss) => {
            crate::fact_tracing::note_non_cacheable_read_fan_out(
                verter_session_query::facts::reuse::NonCacheableReadReason::LeaseMiss,
            );
            None
        }
        _ => None,
    }
}

/// The lowering environment of a recipe escape: every DECLARED type
/// parameter of the owning decl maps to its live-binding identity so the
/// shared lowerer substitutes it — a `ClosedNode` binding maps to its node
/// (the branch-selection oracle then sees the concrete argument), an
/// `Open` / `ClosedAbstract` / FREE parameter maps to its interned
/// `TypeParam` SHELL (so the parameter name cannot be captured by a
/// same-name `name_resolution` entry — declaration parameters shadow the
/// file scope). `bound_params` carries the shells the node walk must treat
/// as CLOSED (`ClosedAbstract` — a closed local with no identity); an
/// `Open`/FREE shell stays out, so the walk judges it an unbound generic.
struct EscapeLoweringEnv {
    env: FxHashMap<String, SemanticNodeId>,
    bound_params: FxHashSet<SemanticNodeId>,
}

fn escape_lowering_env<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    prepared: &PreparedTypeDecl,
    bindings: &ParamBindings,
) -> EscapeLoweringEnv {
    let mut env: FxHashMap<String, SemanticNodeId> = FxHashMap::default();
    let mut bound_params: FxHashSet<SemanticNodeId> = FxHashSet::default();
    for (param, binding) in prepared.type_parameters.iter().zip(bindings.0.iter()) {
        match binding {
            Some(KeyDomainBinding::ClosedNode(id)) => {
                env.insert(param.name.clone(), *id);
            }
            Some(KeyDomainBinding::ClosedAbstract) => {
                let shell = type_param_shell_node(dispatch, prepared, param);
                env.insert(param.name.clone(), shell);
                bound_params.insert(shell);
            }
            Some(KeyDomainBinding::Open) | None => {
                let shell = type_param_shell_node(dispatch, prepared, param);
                env.insert(param.name.clone(), shell);
            }
        }
    }
    EscapeLoweringEnv { env, bound_params }
}

/// The interned `TypeParam` SHELL of one declared parameter — the
/// declaration-identity node an open/abstract parameter lowers to inside a
/// recipe escape (hash-consed: every escape of the same decl version
/// interns the same shell).
fn type_param_shell_node<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    prepared: &PreparedTypeDecl,
    param: &verter_type_expr::facts::NarrowTypeParam,
) -> SemanticNodeId {
    dispatch.graph().intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity {
            canonical_id: prepared.root_identity.canonical_id.clone(),
            owner: prepared.root_identity.owner,
            whole_hash: crate::semantic_query::HashValue::default(),
            decl_name: prepared.root_identity.symbol_name.clone(),
        },
        param_index: u16::try_from(param.ordinal).unwrap_or(u16::MAX),
        constraint: None,
        default: None,
        display_name: OwnedStr::from(param.name.as_str()),
    })
}

/// Shallow-lower ONE derefed authored body under the escape environment —
/// the ONE shared lowerer, structural-transit (carrier-preserving; interior
/// refs intern as `DeclRef`/`InstantiationRef` carriers, never executed).
fn lower_body_under_env<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    prepared: &PreparedTypeDecl,
    slot: &verter_type_expr::locators::TypeBodySlot,
    body: &TypeExpr,
    env: &FxHashMap<String, SemanticNodeId>,
) -> SemanticNodeId {
    // The lowered nodes are scoped at the content version the body was
    // read from: a query over them roots on that version, so it is the
    // same self-root every other read of the file records. A placeholder
    // version would conflict with it and leave every such query
    // unadmittable — recomputed by each evaluation that asks it again.
    let whole_hash = dispatch
        .ctx
        .ensure_indexed_ready_serve(prepared.root_identity.canonical_id.as_ref())
        .map(|serve| serve.indexed.whole_hash)
        .unwrap_or_default();
    let scope = crate::semantic_query::NodeScopeId::File {
        canonical_id: prepared.root_identity.canonical_id.clone(),
        owner: prepared.root_identity.owner,
        whole_hash,
        local_scope: None,
    };
    let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::empty();
    let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
    dispatch.shallow_lower_type_expr_with_context_at_locator(
        body,
        &verter_type_expr::locators::AuthoredBodyLocator::DeclBody(slot.clone()),
        env,
        &scope,
        &prepared.name_resolution,
        None,
        &shadowing,
        &mut substitutions,
        ProjectionReductionContext::structural_transit(),
    )
}

/// ONE registry-owned KEY-DOMAIN closedness rule for a builtin-utility
/// instantiation — consulted by the node-level `__builtin__`
/// `InstantiationRef` arm of [`OpenWalk`] (the sole closedness walker,
/// which the recipe escapes also route through), so the same semantic
/// shape cannot flip verdict by representation route.
///
/// The rule is PER-UTILITY OUTPUT-KEY semantics, owned by the
/// `BuiltinUtility` registry
/// ([`BuiltinUtility::key_domain_argument_positions`]): a utility's
/// produced key domain is closed iff every argument that actually
/// PRODUCES output keys is closed. The object filters (`Pick` / `Omit`)
/// judge the filtered source plus the key-selection argument; the
/// mapped utilities (`Partial` / `Required` / `Readonly`) judge the
/// source; `Record<K, V>` judges ONLY `K` — its value argument never
/// opens the produced key domain (`Omit<Record<'a', T>, 'x'>` stays
/// CLOSED and materialises through the filter). A VALUE-PRODUCING utility (`ReturnType`,
/// `InstanceType`, `Awaited`, `NonNullable`, the union/extraction and
/// string utilities, …) makes NO closed-key claim — its output derives
/// from argument VALUE structure the key-domain argument walk never
/// inspected, so a key-domain-closed argument vector must NOT prove the
/// produced key domain closed (`ReturnType<() => T>` stays a carrier).
/// An under-applied key-producing position is structurally undecidable
/// — not closed. A non-registry name is not a builtin — not closed by
/// this rule.
fn builtin_utility_key_domain_is_closed(decl_name: &str, args: &[KeyDomainBinding]) -> bool {
    use verter_session_query::type_solver::builtin::BuiltinUtility;
    let Some(utility) = BuiltinUtility::from_name(decl_name) else {
        return false;
    };
    let Some(positions) = utility.key_domain_argument_positions() else {
        return false;
    };
    positions
        .iter()
        .all(|&idx| args.get(idx).is_some_and(|binding| !binding.is_open()))
}

/// Whether a KEY-PRODUCING builtin utility's produced key domain is open
/// by an argument the walk judged OPEN at one of its key-producing
/// positions — the filtered source or key selection of `Pick` / `Omit`,
/// the source of `Partial` / `Required` / `Readonly`, the keys of
/// `Record`.
///
/// Reducing such an instantiation cannot close what it produces: an object
/// filter over an open source is the L1 carrier-stop
/// ([`utility_enumeration_domain_is_open_or_unknown`] on the same
/// argument) and reduces to itself, while every other key-producing
/// position becomes the source or key space of the produced mapped type,
/// whose key-domain walk reaches that same open argument. The one-hop
/// reduction behind the registry rule would therefore only answer OPEN —
/// after a demand whose build asks this gate about the argument again, one
/// native evaluation per builtin of a nested chain. A value-producing
/// utility has no key-producing position and is never decided here.
fn builtin_utility_key_domain_is_open(decl_name: &str, args: &[KeyDomainBinding]) -> bool {
    use verter_session_query::type_solver::builtin::BuiltinUtility;
    BuiltinUtility::from_name(decl_name)
        .and_then(BuiltinUtility::key_domain_argument_positions)
        .is_some_and(|positions| {
            positions
                .iter()
                .any(|&idx| args.get(idx).is_some_and(|binding| binding.is_open()))
        })
}

/// Work budget of one openness evaluation
/// ([`utility_enumeration_domain_is_open_or_unknown`] and its siblings):
/// every node visit, declaration proof, recipe and escape the evaluation
/// runs charges it once. The walk visits each node once per position, so
/// it bounds only a pathological already-interned graph: an intersection of
/// thousands of mapped arms, an object of thousands of members, or a
/// heritage chain thousands of declarations deep all decide well inside it.
///
/// It is a WORK bound, not a stack bound: the evaluation runs on the
/// explicit work stack of [`Openness`] at any depth. A deep-but-provably-
/// closed real-world chain that exhausts it (`Unavailable` ⇒ L1
/// carrier-stop) is NOT silently dropped at surface demands: the shallow
/// walker's `Pick`-carrier surface enumeration (`Frame::FlushObjectFilter`)
/// still publishes the picked keys from the source's enumerable arms.
pub const ENUMERATION_DOMAIN_OPENNESS_NODE_BUDGET: u32 = 1 << 16;

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
    if base.canonical_id.as_ref() != "__builtin__" {
        return None;
    }
    enumeration_domain_arg_index_for_name(base.decl_name.as_ref())
}

/// Whether an UNSHADOWED `decl_name` belongs to the L1 object-filter
/// utility family (`Pick` / `Omit`).
///
/// The SINGLE source of L1 family identity — derived from the shared
/// `BuiltinUtility` registry through
/// [`enumeration_domain_arg_index_for_name`], so the lowering entrance
/// (`lower.rs`) and the openness predicate can never diverge on family
/// membership (a one-place edit when the family ever changes). Callers
/// must have ALREADY established the name is unshadowed builtin scope
/// (shadowed names resolve as userland declarations).
pub(in crate::project_semantic_dispatch) fn is_l1_object_filter_utility(decl_name: &str) -> bool {
    enumeration_domain_arg_index_for_name(decl_name).is_some()
}

/// Name-keyed core of [`enumeration_domain_arg_index`] — see that
/// function's family rationale.
fn enumeration_domain_arg_index_for_name(decl_name: &str) -> Option<usize> {
    use verter_session_query::type_solver::builtin::BuiltinUtility;
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
    match BuiltinUtility::from_name(decl_name)? {
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
pub fn utility_enumeration_domain_is_open_or_unknown<
    C: crate::resolver_core::ResolverCapabilities,
>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    base: &DeclIdentity,
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
    Openness::new(dispatch, ENUMERATION_DOMAIN_OPENNESS_NODE_BUDGET)
        .walk(OpenWalk::new(OpenRole::KeyDomainProof, None), domain)
}

/// Shallow-By-Default carrier-stop predicate (L1), MAPPED-TYPE family.
///
/// A mapped type `{ [K in source]: value_expr }` (with optional `as`
/// name-remap) must NOT enumerate its key space and materialise the
/// per-key value under a publication demand when its produced surface
/// still depends on an unbound OUTER generic — doing so degenerates into
/// the per-key value loop over `node_modules` (the `ChatMessagesSlots<T>`
/// / `TableSlots<T>` storm). Returns `true` when ANY of the four mapped
/// inputs is open:
///
/// - the SOURCE or the KEYSPACE key domain (enumeration-domain policy —
///   a finite value surface does NOT open the KEY domain, so `Function` /
///   `Object` leaves stay closed there; only an unbound key space does);
/// - the `as`-clause NAME REMAP, evaluated under the value-body policy
///   (the remapped keys may depend on the outer generic);
/// - the VALUE BODY, evaluated under the value-body policy — the bound
///   mapper binder `K` is treated as CLOSED, finite value surfaces
///   (`Function` / `Object` / `Array` / `Tuple`) are DESCENDED so an
///   outer generic reached through a function parameter or object member
///   still opens the value, and a conditional value inspects BOTH
///   branches (a conditional whose selected branch reaches the outer
///   generic is open).
///
/// CLOSED (returns `false`) ⇒ a finite, outer-generic-free mapped type
/// (`Partial`/`Required`/`Readonly`, `{ [K in keyof Closed]: Closed[K] }`,
/// a K-only transform, a finite keyspace) which still enumerates
/// path-precisely. Pure typed-IR inspection — no reduction, no
/// substitution, no string matching.
pub fn mapped_type_is_open_or_unknown<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    source: SemanticNodeId,
    mapper: &crate::semantic_query::MapperKey,
) -> bool {
    if mapped_type_key_domain_is_open_or_unknown(dispatch, source, mapper) {
        return true;
    }
    // VALUE BODY: does the produced value still depend on an unbound
    // OUTER generic? The bound mapper binder `K` is closed; an intrinsic
    // / helper over `K` (`Capitalize<K>`) or a resolution miss does NOT
    // open it (only an outer-generic argument does).
    Openness::new(dispatch, ENUMERATION_DOMAIN_OPENNESS_NODE_BUDGET).walk(
        OpenWalk::new(OpenRole::OuterGenericValue, Some(mapper.parameter_node)),
        mapper.value_expr,
    )
}

/// Lowering-entrance builtin argument openness: does `node` (a lowered
/// builtin type argument) still depend on unsubstituted generic
/// structure — an unbound `TypeParam` (including a mapper binder whose
/// per-key substitution happens later at a demand point), an open
/// operator carrier over one, an `Opaque` degradation, or an
/// undecidable walk?
///
/// The carrier-mode lowering gate (`Navigate` / `Skeleton` / `Shallow`)
/// consults this per argument: a builtin instantiation over an OPEN
/// argument must intern the `InstantiationRef` carrier instead of
/// executing eagerly — eager execution over carrier-shaped args bakes
/// `Opaque(Miss)` into the produced structure (a
/// `NonNullable<ChatSlots[K]>` conditional check with unbound `K`)
/// and destroys the deferred structure the demand points need for
/// per-key realization. Closed-argument builtins keep the eager
/// execute, byte-for-byte.
pub fn builtin_lowering_argument_is_open<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    node: SemanticNodeId,
) -> bool {
    Openness::new(dispatch, ENUMERATION_DOMAIN_OPENNESS_NODE_BUDGET)
        .walk(OpenWalk::new(OpenRole::OuterGenericValue, None), node)
}

/// KEY-PRODUCTION axis of [`mapped_type_is_open_or_unknown`]: is the
/// mapped type's produced KEY SET open or undecidable, judging ONLY the
/// source / keyspace / `as`-remap (binder-bound KEY-DOMAIN policy) and
/// never the value body?
///
/// The empty-path Shallow surface enumerator gates per-key enumeration
/// on THIS axis alone: a mapped type with a CLOSED key domain and an
/// open VALUE body (`{ [K in keyof ChatSlots]?: … MB<T> … }`) still
/// enumerates its keys path-precisely — the per-key VALUES materialise
/// under `StructuralTransit(Navigate)` and keep the open generic as a
/// deferred carrier (shallow values). Value-body openness defers only
/// the operator MATERIALISATION routes (the full predicate above), not
/// the key enumeration.
pub fn mapped_type_key_domain_is_open_or_unknown<C: crate::resolver_core::ResolverCapabilities>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    source: SemanticNodeId,
    mapper: &crate::semantic_query::MapperKey,
) -> bool {
    // KEY DOMAIN: is the produced KEY SPACE open or undecidable (making
    // `keyof source` non-enumerable — the storm)? A mapped source / key
    // space with a CLOSED key domain is always enumerable at build time —
    // a finite object surface, a closed builtin-utility instantiation
    // (`Omit<MenuProps, …>`), a closed conditional, a finite literal
    // union — so it must NOT carrier-stop (`Partial`/`Required`/`Readonly`
    // over a closed utility source still enumerate). The key-domain walk
    // asks the key-space-finiteness question (so an unresolved carrier
    // head / a resolution miss / a recursive ref is OPEN — the keys cannot
    // be enumerated), does NOT descend the source's member VALUES (a
    // member value reaching `T` does not change WHICH keys exist: `{ [K in
    // keyof {a: Foo<T>}]: V }` still has the finite key `a`), and judges a
    // resolved instantiation in the domain by the SAME per-argument
    // key-domain closure rule as `Pick`/`Omit`
    // (`prepared_instantiation_key_domain_is_closed`): an open argument
    // confined to member VALUE positions of a fixed-key body keeps the key
    // domain CLOSED, so `{ [K in keyof Foo<T>]: string }` over `interface
    // Foo<T> { label?: string; items?: T }` still enumerates `label` /
    // `items`.
    //
    // The three key-domain operands split BY ROLE. The SOURCE and the key
    // SPACE are [`OpenRole::KeyDomainProof`] — they MUST PROVE finiteness.
    // A value-producing builtin source (`ReturnType<…>`) makes no
    // closed-key claim from its argument bindings, so the registry rule
    // alone cannot prove it; the walk then falls back to a bounded ONE-HOP
    // reduction of the instantiation and classifies the REDUCED surface
    // (`Openness::reduced_instantiation`), staying
    // OPEN on a partial demand, a no-progress carrier-stop, or an open
    // reduced surface. The `as`-REMAP is
    // [`OpenRole::MappedNameRemap`] — a K-only transform over the bound
    // binder (`as keyof Foo<T>` over a fixed-key `Foo`, `as Capitalize<K>`)
    // is decidable per key and stays CLOSED, while a direct outer-generic
    // remap (`as T`) or an open-argument instantiation stays OPEN. The role
    // is part of every verdict identity (`memo`/`in_flight` are role-keyed),
    // so the proof and remap walks cannot alias a verdict; the proof walk's
    // remaining budget is threaded into the remap walk so the whole
    // classification stays globally bounded.
    Openness::new(dispatch, ENUMERATION_DOMAIN_OPENNESS_NODE_BUDGET)
        .run(Ask::Mapped(MappedOperands::new(
            FxHashSet::default(),
            source,
            mapper,
            None,
        )))
        .open()
}

/// The ROLE an [`OpenWalk`] plays — the SINGLE policy axis. Every other
/// policy attribute (the [`OpenQuestion`], whether finite value surfaces
/// are descended, how an `InstantiationRef` is judged, and whether a
/// concrete no-open-argument instantiation is taken closed without a
/// finiteness proof) is DERIVED from the role, so an invalid combination
/// (e.g. the key-domain concrete-instantiation shortcut on a source that
/// must prove finiteness) cannot be expressed.
///
/// The three roles are the three policies the walker actually runs:
///
/// - [`OpenRole::KeyDomainProof`] — the `Pick`/`Omit` enumeration domain
///   AND the mapped SOURCE / key SPACE. It asks the key-space-finiteness
///   question ([`OpenQuestion::KeyDomain`]) and MUST PROVE finiteness: a
///   concrete no-open-argument instantiation is NOT taken closed by
///   shortcut — it falls through to the builtin registry
///   (`builtin_utility_key_domain_is_closed`) / prepared-decl proof, and a
///   value-producing builtin (`ReturnType`, `InstanceType`, the string
///   intrinsics, …) the registry cannot prove closed is decided by the
///   bounded one-hop reduction fallback
///   ([`Openness::reduced_instantiation`]): CLOSED
///   only when the REDUCED surface is. An
///   instantiation is judged per-argument
///   (`per_argument_key_domain = true`): an open argument confined to
///   member VALUE positions of a fixed-key body keeps the key domain
///   CLOSED. Finite value surfaces are closed leaves
///   (`descend_value_surfaces = false`).
/// - [`OpenRole::MappedNameRemap`] — the mapped `as`-clause name transform.
///   It also asks [`OpenQuestion::KeyDomain`] and judges per-argument, but
///   a CONCRETE no-open-argument instantiation IS taken closed without a
///   separate finiteness proof: the produced key set is concrete at build
///   time and the mapped key ENUMERATOR owns it, so a K-only transform over
///   the BOUND binder `K` (`as `on${Capitalize<K>}``, `as Capitalize<K>`)
///   is decidable per key. (The per-key remap classifier still validates
///   the produced names downstream.) Finite value surfaces are closed
///   leaves.
/// - [`OpenRole::OuterGenericValue`] — the mapped VALUE body AND the
///   lowering-entrance builtin-argument gate. It asks the NARROWER
///   [`OpenQuestion::OuterGenericReachability`] ("does this reach an unbound
///   OUTER generic?"), judges an instantiation open iff an ARGUMENT is open
///   (`per_argument_key_domain = false`), and DESCENDS finite value surfaces
///   (`descend_value_surfaces = true`) to find an outer generic nested in a
///   member value / parameter / element.
///
/// The role is part of EVERY verdict identity: both `memo` and `in_flight`
/// are keyed `(SemanticNodeId, OperandPosition, OpenRole)`, so a node
/// reached under two different roles cannot alias one role's cached verdict
/// (or one role's in-flight back-edge) to the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum OpenRole {
    /// `Pick`/`Omit` enumeration domain + mapped source / key space — MUST
    /// prove finiteness (no concrete-instantiation shortcut).
    KeyDomainProof,
    /// Mapped `as` name transform — K-only transforms over the bound binder
    /// are taken closed (the concrete-instantiation shortcut applies).
    MappedNameRemap,
    /// Mapped value body + lowering-entrance builtin argument —
    /// outer-generic-reachability question, value surfaces descended.
    OuterGenericValue,
}

impl OpenRole {
    /// The openness QUESTION this role asks (the undecidable-answer
    /// direction). Key-domain roles ask [`OpenQuestion::KeyDomain`]; the
    /// value role asks [`OpenQuestion::OuterGenericReachability`].
    fn question(self) -> OpenQuestion {
        match self {
            OpenRole::KeyDomainProof | OpenRole::MappedNameRemap => OpenQuestion::KeyDomain,
            OpenRole::OuterGenericValue => OpenQuestion::OuterGenericReachability,
        }
    }

    /// Whether finite value surfaces (`Object`/`Function`/`Array`/`Tuple`)
    /// are DESCENDED to find a nested outer generic. Only the value role
    /// descends; the key-domain roles treat them as closed leaves (a member
    /// value does not change WHICH keys a surface exposes).
    fn descend_value_surfaces(self) -> bool {
        matches!(self, OpenRole::OuterGenericValue)
    }

    /// Whether an `InstantiationRef` is judged by the per-argument
    /// key-domain closure rule (`true` — key-domain roles) or simply
    /// "does any argument reach the outer generic" (`false` — the value
    /// role).
    fn per_argument_key_domain(self) -> bool {
        matches!(self, OpenRole::KeyDomainProof | OpenRole::MappedNameRemap)
    }

    /// Whether a CONCRETE (no-open-argument) instantiation in a per-argument
    /// key-domain position is taken CLOSED WITHOUT a separate finiteness
    /// proof. ONLY the `as`-remap role takes this shortcut: the produced key
    /// set is concrete at build time and the mapped key enumerator owns it
    /// (a `Capitalize<K>` over the bound binder is decidable per key). The
    /// `KeyDomainProof` role must PROVE finiteness — it falls through to the
    /// builtin registry / prepared-decl proof, and a builtin the registry
    /// cannot prove closed (a value-producing `ReturnType<…>`) is decided by
    /// the bounded one-hop reduction fallback, never by this shortcut.
    fn concrete_no_open_arg_is_closed(self) -> bool {
        matches!(self, OpenRole::MappedNameRemap)
    }
}

/// The openness QUESTION an [`OpenWalk`] answers — the undecidable-answer
/// direction, DERIVED from the walk's [`OpenRole`].
///
/// The two variants are the two undecidable-answer directions the roles
/// need:
///
/// - [`OpenQuestion::KeyDomain`] — "is the KEY SPACE undecidable / not
///   provably finite?" An undecidable node (a resolution miss, a degraded
///   `Opaque`, an unresolved head, budget exhaustion) is OPEN, because the
///   keys cannot be enumerated (conservative). This is the
///   `Pick`/`Omit` enumeration domain AND the mapped source / key space /
///   `as`-remap policy.
/// - [`OpenQuestion::OuterGenericReachability`] — "does this reach an
///   UNBOUND OUTER generic?" An undecidable node carries NO outer generic
///   (a missing node, an unresolved head, a `DeclRef` whose own type
///   parameters cannot be the mapper's outer generic), so it is CLOSED for
///   this question. This is the [`OpenRole::OuterGenericValue`] question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OpenQuestion {
    /// Key-space-finiteness question (the `KeyDomainProof` /
    /// `MappedNameRemap` roles).
    KeyDomain,
    /// Outer-generic-reachability question (the `OuterGenericValue` role).
    OuterGenericReachability,
}

impl OpenQuestion {
    /// The question-correct answer for an UNDECIDABLE node (a resolution
    /// miss, a degraded `Opaque`, an unresolved carrier head, or budget
    /// exhaustion): OPEN for the key-domain question (the keys are not
    /// provably finite), CLOSED for the outer-generic question (an
    /// undecidable node carries no outer generic).
    fn undecidable_is_open(self) -> bool {
        matches!(self, OpenQuestion::KeyDomain)
    }
}

/// One walk's policy and verdict state: the [`OpenRole`], the binders the
/// walk treats as CLOSED locals, and the role/position-qualified `memo` /
/// `in_flight` sets of the nodes it has visited.
///
/// A walk is the scope of one question. The root predicates open one; a
/// nested mapped type opens one per operand role with its own binder BOUND
/// (the binder scope is local to that node, so verdicts computed under the
/// extended bind set must not leak into the outer walk's memo, and vice
/// versa); a recipe escape opens one to classify its lowered node. Every
/// walk of one evaluation shares the evaluation's budget.
///
/// The role is part of EVERY verdict identity: both `memo` and `in_flight`
/// are keyed `(SemanticNodeId, OperandPosition, OpenRole)`, so a node
/// reached under two different roles cannot alias one role's cached verdict
/// (or one role's in-flight back-edge) to the other.
struct OpenWalk {
    /// The SINGLE policy axis. The [`OpenQuestion`], value-surface descent,
    /// per-argument key-domain judgement, and the concrete-instantiation
    /// shortcut are all DERIVED from this role (see [`OpenRole`]) — there
    /// are no independent policy bools to fall out of sync.
    role: OpenRole,
    bound_params: FxHashSet<SemanticNodeId>,
    in_flight: FxHashSet<OpenMemoKey>,
    memo: FxHashMap<OpenMemoKey, bool>,
}

impl OpenWalk {
    /// A walk for `role` with `bound_param` (a mapper binder) BOUND, and
    /// fresh memo / in-flight state.
    fn new(role: OpenRole, bound_param: Option<SemanticNodeId>) -> Self {
        Self::with_bound(role, bound_param.into_iter().collect())
    }

    fn with_bound(role: OpenRole, bound_params: FxHashSet<SemanticNodeId>) -> Self {
        Self {
            role,
            bound_params,
            in_flight: FxHashSet::default(),
            memo: FxHashMap::default(),
        }
    }
}

/// Verdict identity for an [`OpenWalk`] node visit: the node, the operand
/// position it was reached at, AND the walk's [`OpenRole`]. The role is
/// load-bearing in BOTH `memo` and `in_flight`: a node reached under
/// `KeyDomainProof` and under `MappedNameRemap` has two DIFFERENT verdict
/// policies, so sharing a key would either alias one role's cached verdict
/// to the other (memo) or report a cross-role revisit as a fake cycle
/// (`in_flight`, the closed-for-revisit answer). Both maps use this key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct OpenMemoKey {
    node: SemanticNodeId,
    position: OperandPosition,
    role: OpenRole,
}

/// What a finished question tells the frame that asked it.
#[derive(Debug, Clone, Copy)]
enum Answer {
    /// A node visit's openness.
    Open(bool),
    /// A closedness proof's verdict.
    Verdict(ClosednessVerdict),
}

impl Answer {
    fn open(self) -> bool {
        match self {
            Answer::Open(open) => open,
            Answer::Verdict(_) => unreachable!("an openness frame resumes with an openness"),
        }
    }

    fn verdict(self) -> ClosednessVerdict {
        match self {
            Answer::Verdict(verdict) => verdict,
            Answer::Open(_) => unreachable!("a closedness frame resumes with a verdict"),
        }
    }
}

/// A question the machine evaluates before the frame that asked it resumes.
enum Ask {
    /// Is `node`, reached at `position`, open under the current walk?
    Visit {
        node: SemanticNodeId,
        position: OperandPosition,
    },
    /// Is `node` open at `position` under the fresh walk `scope`? The scope
    /// ends with the answer.
    Walk {
        scope: OpenWalk,
        node: SemanticNodeId,
        position: OperandPosition,
    },
    /// The KEY-production operands of a mapped type, each under its own
    /// role's walk, then its value body where the surrounding walk consumes
    /// values.
    Mapped(MappedOperands),
    /// Does the bare declaration `(canonical_id, owner, name)` have a
    /// closed key domain?
    Decl {
        canonical_id: OwnedStr,
        owner: verter_type_expr::TopLevelOwnerId,
        name: OwnedStr,
    },
    /// Does `base<args…>` have a closed key domain?
    Instantiation {
        base: DeclIdentity,
        args: Vec<KeyDomainBinding>,
    },
    /// Do all of `recipes`, evaluated in order under `env`, close?
    Recipes {
        env: Rc<RecipeEnv>,
        recipes: Arc<[ClosednessRecipe]>,
    },
}

/// One step of the machine: an answer for the frame below, or a further
/// question to evaluate first.
enum Flow {
    Answer(Answer),
    Ask(Ask),
}

impl Flow {
    fn open(open: bool) -> Self {
        Flow::Answer(Answer::Open(open))
    }

    fn verdict(verdict: ClosednessVerdict) -> Self {
        Flow::Answer(Answer::Verdict(verdict))
    }

    fn visit(node: SemanticNodeId, position: OperandPosition) -> Self {
        Flow::Ask(Ask::Visit { node, position })
    }
}

/// The operands of one mapped type, judged in order — source, key space,
/// `as`-remap, value body — stopping at the first open one.
struct MappedOperands {
    /// The binders BOUND around the mapped type; its own binder joins them
    /// for every operand.
    bound: FxHashSet<SemanticNodeId>,
    source: SemanticNodeId,
    key_space: SemanticNodeId,
    name_remap: Option<SemanticNodeId>,
    value_expr: SemanticNodeId,
    /// The role and position the value body is judged at, when the
    /// surrounding walk consumes the mapped type's values.
    value: Option<(OpenRole, OperandPosition)>,
    stage: MappedStage,
}

impl MappedOperands {
    fn new(
        mut bound: FxHashSet<SemanticNodeId>,
        source: SemanticNodeId,
        mapper: &crate::semantic_query::MapperKey,
        value: Option<(OpenRole, OperandPosition)>,
    ) -> Self {
        bound.insert(mapper.parameter_node);
        Self {
            bound,
            source,
            key_space: mapper.key_space,
            name_remap: mapper.name_remap,
            value_expr: mapper.value_expr,
            value,
            stage: MappedStage::Source,
        }
    }
}

/// The operand a [`Frame::Mapped`] awaits.
#[derive(Clone, Copy)]
enum MappedStage {
    Source,
    KeySpace,
    NameRemap,
    Value,
}

/// The operand a [`Frame::Conditional`] awaits.
#[derive(Clone, Copy)]
enum ConditionalStage {
    Check,
    Extends,
    TrueBranch,
}

/// The unfilled defaulted parameters of an instantiation, verified in
/// declaration order before its body recipes run.
struct Defaults {
    env: RecipeEnv,
    recipes: Arc<[ClosednessRecipe]>,
    /// The parameter whose default is being verified.
    next: usize,
    /// The lowered default of `next`, awaiting its classification.
    default_node: SemanticNodeId,
}

/// A suspended question, resumed with the answer of the question it asked.
enum Frame<'a> {
    /// Settle a visited node's verdict into its walk.
    Settle(OpenMemoKey),
    /// Open as soon as one of `children` is; `otherwise` when none is.
    AnyOpen {
        children: std::vec::IntoIter<(SemanticNodeId, OperandPosition)>,
        otherwise: bool,
    },
    /// End the innermost walk, passing its answer through.
    EndWalk,
    /// A node is open unless the proof it asked for proves it closed.
    NotClosed,
    /// The answer of the declaration or instantiation proof whose in-flight
    /// entry this frame releases, and which it records for reuse.
    LeaveProof(Box<ProofSpan<'a>>),
    /// A recipe escape's classified node: closed, open, or — open with the
    /// budget exhausted — undecided.
    Classified,
    /// A conditional kept whole: its operands, then both branches.
    Conditional {
        data: crate::semantic_query_memo::NodeData,
        position: OperandPosition,
        stage: ConditionalStage,
    },
    Mapped(MappedOperands),
    /// An instantiation's arguments, each judged in turn (no short circuit:
    /// a closed argument keeps its node identity for the binding).
    InstantiationArgs {
        node: SemanticNodeId,
        position: OperandPosition,
        base: DeclIdentity,
        args: Arc<[SemanticNodeId]>,
        bindings: Vec<KeyDomainBinding>,
    },
    /// An unresolved carrier whose open argument did not open it: resolve
    /// its head next.
    CarrierHead {
        node: SemanticNodeId,
        position: OperandPosition,
    },
    /// The remaining recipes of one declaration.
    Recipes {
        env: Rc<RecipeEnv>,
        recipes: Arc<[ClosednessRecipe]>,
        next: usize,
    },
    /// An indexed access's INDEX operand, after its object closed.
    ProjectionIndex {
        env: Rc<RecipeEnv>,
        index: TypeBodySlot,
    },
    Defaults(Defaults),
}

/// One proof in flight on a [`ClosednessChain`].
#[derive(Debug)]
struct ChainLink {
    /// Tells this proof apart from every other one entered at the same
    /// position of the chain.
    push: u64,
    /// The positions below this one whose in-flight cycle refusal the proof
    /// observed — directly or through a proof it ran or reused.
    cycle_dependencies: SmallVec<[usize; 2]>,
}

/// The positions (and pushes) of the in-flight proofs a completed proof's
/// cycle refusals depended on.
type CycleDependencies = SmallVec<[(usize, u64); 2]>;

/// The closedness proofs in flight on one dispatcher, innermost last, and
/// the context-dependent refusals its evaluations have met.
///
/// A proof that refuses a back-edge to a proof BELOW it answers for that
/// context only: run where the lower proof is not in flight, it would not
/// refuse. The chain records such a dependency on every proof between the
/// two, so a completed proof is reused only while each proof it depended on
/// is still in flight at the same position ([`Self::holds`]). A refusal of
/// the proof itself, or of one entered within it, is its own and reused
/// freely.
#[derive(Debug, Default)]
pub(in crate::project_semantic_dispatch) struct ClosednessChain {
    active: indexmap::IndexSet<
        crate::project_semantic_dispatch::InstantiateIdentity,
        rustc_hash::FxBuildHasher,
    >,
    links: Vec<ChainLink>,
    pushes: u64,
    /// Refusals that depend on more than a proof's inputs — an exhausted
    /// budget, an unfinished dispatcher read, a carrier already being
    /// normalized. A proof during which this moves is never reused.
    context_refusals: u64,
}

impl ClosednessChain {
    /// Enter the proof of `identity`: `false` — and a cycle dependency for
    /// every proof above the refused one — when it is already in flight.
    fn enter(&mut self, identity: crate::project_semantic_dispatch::InstantiateIdentity) -> bool {
        if let Some(position) = self.active.get_index_of(&identity) {
            let top = self.links.len() - 1;
            if position < top {
                insert_dependency(&mut self.links[top].cycle_dependencies, position);
            }
            return false;
        }
        self.active.insert(identity);
        self.pushes += 1;
        self.links.push(ChainLink {
            push: self.pushes,
            cycle_dependencies: SmallVec::new(),
        });
        true
    }

    /// Leave the innermost proof: its cycle dependencies, each of which
    /// below its parent becomes the parent's too.
    fn leave(&mut self) -> CycleDependencies {
        self.active.pop();
        let Some(link) = self.links.pop() else {
            return CycleDependencies::new();
        };
        let parent = self.links.len().checked_sub(1);
        let mut dependencies = CycleDependencies::new();
        for position in link.cycle_dependencies {
            dependencies.push((position, self.links[position].push));
            if let Some(parent) = parent.filter(|&parent| position < parent) {
                insert_dependency(&mut self.links[parent].cycle_dependencies, position);
            }
        }
        dependencies
    }

    /// Whether every proof `dependencies` names is still in flight where it
    /// was.
    fn holds(&self, dependencies: &CycleDependencies) -> bool {
        dependencies.iter().all(|&(position, push)| {
            self.links
                .get(position)
                .is_some_and(|link| link.push == push)
        })
    }

    /// A reused proof's cycle dependencies become the innermost proof's.
    fn observe(&mut self, dependencies: &CycleDependencies) {
        let Some(top) = self.links.len().checked_sub(1) else {
            return;
        };
        for &(position, _) in dependencies {
            if position < top {
                insert_dependency(&mut self.links[top].cycle_dependencies, position);
            }
        }
    }
}

fn insert_dependency(dependencies: &mut SmallVec<[usize; 2]>, position: usize) {
    if !dependencies.contains(&position) {
        dependencies.push(position);
    }
}

/// The question a completed closedness proof answers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ClosednessProofKey {
    Decl(crate::project_semantic_dispatch::InstantiateIdentity),
    Instantiation(
        crate::project_semantic_dispatch::InstantiateIdentity,
        Arc<[KeyDomainBinding]>,
    ),
}

/// A closedness proof completed on the dispatch transaction: its verdict,
/// the work it charged, what it depended on, and what reusing it replays.
#[derive(Debug)]
struct CompletedProof {
    verdict: ClosednessVerdict,
    /// The budget the proof charged its evaluation; a reuse charges the
    /// same, so an evaluation refuses for budget exactly where running the
    /// proof again would.
    cost: u32,
    /// What the proof's dispatcher reads cost the connected demand it ran
    /// in — `None` when it ran in none. A reuse inside a demand replays it
    /// first, so the demand pays what running the proof would and stops
    /// where running it would: a demand that cannot pay runs the proof.
    receipt: Option<Arc<crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt>>,
    cycle_dependencies: CycleDependencies,
    /// A canonical names no content version: the proof answers only under
    /// the edit clocks it ran under.
    served_at: crate::project_semantic_dispatch::WorkspaceEdit,
    reuse: verter_session_query::facts::reuse::ReuseClass,
    replay: crate::project_semantic_dispatch::dispatch_txn::FlowMemberReuse,
}

/// The closedness proofs completed on one dispatch transaction — the reuse
/// that keeps a proof from being re-derived by every evaluation a dispatcher
/// query re-enters. Only a proof that met no context-dependent refusal and
/// whose reads an enclosing build could observe enters; nothing is evicted.
#[derive(Default)]
pub(in crate::project_semantic_dispatch) struct ClosednessProofTable {
    proofs: FxHashMap<ClosednessProofKey, Arc<CompletedProof>>,
}

impl std::fmt::Debug for ClosednessProofTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClosednessProofTable")
            .field("proofs", &self.proofs.len())
            .finish()
    }
}

impl ClosednessProofTable {
    fn get(
        &self,
        key: &ClosednessProofKey,
        at: crate::project_semantic_dispatch::WorkspaceEdit,
    ) -> Option<Arc<CompletedProof>> {
        self.proofs
            .get(key)
            .filter(|proof| proof.served_at == at)
            .cloned()
    }

    fn complete(&mut self, key: ClosednessProofKey, proof: CompletedProof) {
        if proof.reuse.is_request_reusable() {
            self.proofs.insert(key, Arc::new(proof));
        }
    }
}

/// A closedness proof being evaluated: what [`Frame::LeaveProof`] needs to
/// record it when it answers.
struct ProofSpan<'a> {
    key: ClosednessProofKey,
    recording: Option<crate::project_semantic_dispatch::build::TransactionReadRecording<'a>>,
    /// Whether the proof records its cost on the connected demand it runs
    /// in: a recording opened for it there, sealed into its receipt when
    /// the proof is kept and abandoned to its consumer otherwise.
    costed: bool,
    budget: u32,
    context_refusals: u64,
    served_at: crate::project_semantic_dispatch::WorkspaceEdit,
}

/// The openness / closedness machine described in the module docs.
///
/// A question either answers at once or pushes the [`Frame`] that resumes
/// with its answer, so the native stack stays constant at any depth. The
/// node visits, every walk opened for a nested scope, and the declaration,
/// instantiation and recipe proofs all run on `frames`; `walks` holds the
/// open walk scopes, innermost last.
struct Openness<'a, C: crate::resolver_core::ResolverCapabilities> {
    /// The ACTIVE dispatcher (NOT a freshly-constructed one). The machine
    /// resolves carrier heads and consults the shared branch-selection
    /// oracle THROUGH this dispatcher so the dispatcher-local
    /// `instantiate_active` + `carrier_normalizing` cycle-guard state and
    /// the dispatch-wide in-flight closedness set are shared — a
    /// freshly-constructed dispatch would lose that state and diverge on
    /// recursive refs.
    dispatch: &'a ProjectSemanticDispatch<'a, C>,
    /// The work budget every walk and proof of this evaluation charges.
    budget: u32,
    walks: Vec<OpenWalk>,
    frames: Vec<Frame<'a>>,
}

impl<C: crate::resolver_core::ResolverCapabilities> Drop for Openness<'_, C> {
    /// An evaluation abandoned mid-proof (an unwinding panic) releases the
    /// dispatch-wide in-flight entries its frames still hold, innermost
    /// first, so the dispatcher's cycle guard never refuses a declaration no
    /// proof is evaluating; the abandoned proofs record nothing.
    fn drop(&mut self) {
        while let Some(frame) = self.frames.pop() {
            if let Frame::LeaveProof(span) = frame {
                self.dispatch.closedness.borrow_mut().leave();
                if span.costed {
                    self.dispatch.connected_demand.abandon_cost_scope();
                }
                drop(span);
            }
        }
    }
}

impl<'a, C: crate::resolver_core::ResolverCapabilities> Openness<'a, C> {
    fn new(dispatch: &'a ProjectSemanticDispatch<'a, C>, budget: u32) -> Self {
        Self {
            dispatch,
            budget,
            walks: Vec::new(),
            frames: Vec::new(),
        }
    }

    /// Whether `node` is open at the key-domain position under `scope`.
    fn walk(&mut self, scope: OpenWalk, node: SemanticNodeId) -> bool {
        self.run(Ask::Walk {
            scope,
            node,
            position: OperandPosition::KeyDomain,
        })
        .open()
    }

    /// Evaluate `ask` to its answer.
    fn run(&mut self, ask: Ask) -> Answer {
        let mut flow = self.ask(ask);
        loop {
            match flow {
                Flow::Ask(ask) => flow = self.ask(ask),
                Flow::Answer(answer) => {
                    let Some(frame) = self.frames.pop() else {
                        return answer;
                    };
                    flow = self.resume(frame, answer);
                }
            }
        }
    }

    /// Charge one unit of work; `false` when the budget is spent — a
    /// refusal of this evaluation's own, under which no proof in flight is
    /// recorded for reuse.
    fn charge(&mut self) -> bool {
        if self.budget == 0 {
            self.context_refusal();
            return false;
        }
        self.budget -= 1;
        true
    }

    /// Record a refusal that depends on more than the inputs of the proofs
    /// in flight, so none of them is recorded for reuse.
    fn context_refusal(&self) {
        self.dispatch.closedness.borrow_mut().context_refusals += 1;
    }

    fn current(&mut self) -> &mut OpenWalk {
        self.walks
            .last_mut()
            .expect("a node visit runs inside a walk")
    }

    /// The question-correct answer for an UNDECIDABLE node under the current
    /// walk.
    fn undecidable(&mut self) -> bool {
        self.current().role.question().undecidable_is_open()
    }

    fn ask(&mut self, ask: Ask) -> Flow {
        match ask {
            Ask::Visit { node, position } => self.visit(node, position),
            Ask::Walk {
                scope,
                node,
                position,
            } => {
                self.walks.push(scope);
                self.frames.push(Frame::EndWalk);
                self.visit(node, position)
            }
            Ask::Mapped(operands) => self.mapped_operands(operands),
            Ask::Decl {
                canonical_id,
                owner,
                name,
            } => {
                let identity = (canonical_id.clone(), owner, name.clone());
                if let Some(answer) = self.enter_proof(ClosednessProofKey::Decl(identity)) {
                    return answer;
                }
                self.decl(&canonical_id, owner, &name)
            }
            Ask::Instantiation { base, args } => {
                let identity = (
                    base.canonical_id.clone(),
                    base.owner,
                    base.decl_name.clone(),
                );
                let key = ClosednessProofKey::Instantiation(identity, Arc::from(args.as_slice()));
                if let Some(answer) = self.enter_proof(key) {
                    return answer;
                }
                self.instantiation(base, args)
            }
            Ask::Recipes { env, recipes } => self.recipes(env, recipes, 0),
        }
    }

    /// Enter the proof `key` names: its refusal when it is already in
    /// flight, its reused verdict when this transaction completed it, or —
    /// `None` — the frame that records it once it answers.
    ///
    /// The in-flight set is the DISPATCH-WIDE cycle guard, NOT a permanent
    /// visited set: the entry leaves with the proof's answer, so two SIBLING
    /// references to the same declaration (a diamond — `Foo & Bar` both
    /// reaching `Shared`) are each judged on their merits; only a genuine
    /// back-edge on the current closedness chain (true recursion, including
    /// one that round-trips through a recipe escape's lowering and a gate a
    /// dispatcher query re-enters) refuses — UNAVAILABLE, not a proof of
    /// openness. An instantiation shares its declaration's entry: a
    /// key-reachable self-instantiation is a genuine recursion.
    ///
    /// A completed proof is reused only where running it again would answer
    /// the same: under the edit clocks it ran under, while every in-flight
    /// proof its cycle refusals depended on still is, with the budget to pay
    /// what it charged, and — inside a connected demand — once the demand
    /// has paid the receipt of the reads it issued, whose footprint must fit
    /// the demand's allowances. Reusing it replays its reads into the live
    /// scopes and its cycle dependencies into the proof in flight, exactly
    /// as running it would; a demand that cannot pay runs the proof, and
    /// meets the refusal running it meets.
    fn enter_proof(&mut self, key: ClosednessProofKey) -> Option<Flow> {
        let identity = match &key {
            ClosednessProofKey::Decl(identity) | ClosednessProofKey::Instantiation(identity, _) => {
                identity.clone()
            }
        };
        if !self.dispatch.closedness.borrow_mut().enter(identity) {
            return Some(Flow::verdict(ClosednessVerdict::Unavailable));
        }
        let served_at = self.dispatch.workspace_edit();
        let completed = self
            .dispatch
            .dispatch_txn
            .borrow()
            .closedness_proofs
            .get(&key, served_at)
            .filter(|proof| {
                proof.cost <= self.budget
                    && self
                        .dispatch
                        .closedness
                        .borrow()
                        .holds(&proof.cycle_dependencies)
            });
        if let Some(proof) = completed.filter(|proof| self.admits_reuse(proof)) {
            self.dispatch.closedness.borrow_mut().leave();
            self.budget -= proof.cost;
            self.dispatch
                .replay_transaction_read(&proof.reuse, &proof.replay);
            self.dispatch
                .closedness
                .borrow_mut()
                .observe(&proof.cycle_dependencies);
            return Some(Flow::verdict(proof.verdict));
        }
        let recording = self.dispatch.begin_transaction_read();
        let costed = recording.is_some() && self.open_proof_cost_scope(&key);
        let span = ProofSpan {
            key,
            recording,
            costed,
            budget: self.budget,
            context_refusals: self.dispatch.closedness.borrow().context_refusals,
            served_at,
        };
        self.frames.push(Frame::LeaveProof(Box::new(span)));
        None
    }

    /// Whether the active connected demand, if any, pays for reusing
    /// `proof`: its receipt replays into the demand. A proof that ran in no
    /// demand has no receipt to replay, so it serves only outside one.
    fn admits_reuse(&self, proof: &CompletedProof) -> bool {
        let ledger = &self.dispatch.connected_demand;
        if !ledger.is_active() {
            return true;
        }
        proof.receipt.as_ref().is_some_and(|receipt| {
            matches!(
                self.dispatch.admit_served(
                    receipt,
                    crate::project_semantic_dispatch::cost_receipt::Nesting::InPlace
                ),
                crate::project_semantic_dispatch::ServedAdmission::Admitted
            )
        })
    }

    /// Open the recording of the proof `key`'s cost on the active connected
    /// demand: every read the proof issues is a prerequisite of its receipt.
    /// `false` outside a demand, or when the proof's operands cannot be
    /// rooted for its identity.
    fn open_proof_cost_scope(&self, key: &ClosednessProofKey) -> bool {
        let ledger = &self.dispatch.connected_demand;
        if !ledger.is_active() {
            return false;
        }
        let operands: Vec<SemanticNodeId> = match key {
            ClosednessProofKey::Decl(_) => Vec::new(),
            ClosednessProofKey::Instantiation(_, bindings) => bindings
                .iter()
                .filter_map(|binding| match binding {
                    KeyDomainBinding::ClosedNode(node) => Some(*node),
                    KeyDomainBinding::Open | KeyDomainBinding::ClosedAbstract => None,
                })
                .collect(),
        };
        let Ok(roots) = self.dispatch.graph().root_nodes(operands) else {
            return false;
        };
        ledger.open_cost_scope(
            crate::project_semantic_dispatch::cost_receipt::CostIdentity::of_key(key.clone())
                .with_roots(roots),
        );
        true
    }

    /// Leave the proof `span` records, which answered `verdict`, and record
    /// it for reuse when nothing but its inputs decided it: no
    /// context-dependent refusal, cancellation or edit while it ran, and
    /// reads an enclosing build observes completely.
    fn leave_proof(&mut self, span: ProofSpan<'a>, verdict: ClosednessVerdict) {
        let cycle_dependencies = self.dispatch.closedness.borrow_mut().leave();
        let ProofSpan {
            key,
            recording,
            costed,
            budget,
            context_refusals,
            served_at,
        } = span;
        let ledger = &self.dispatch.connected_demand;
        let Some(recording) = recording else {
            return;
        };
        let undisturbed = self.dispatch.closedness.borrow().context_refusals == context_refusals
            && !self.dispatch.cancellation.is_cancelled()
            && self.dispatch.workspace_edit() == served_at;
        let Some((reuse, replay)) = recording.finish(self.dispatch, undisturbed) else {
            if costed {
                ledger.abandon_cost_scope();
            }
            return;
        };
        let receipt = if costed {
            let request = crate::request_context::current_request_budget();
            let Some(receipt) = ledger.seal_cost_scope(request.as_deref()) else {
                return;
            };
            // What consumed the proof — an enclosing proof, or the build the
            // evaluation runs in — owes it, as it owes a query it read.
            ledger.record_prerequisite(
                &receipt,
                crate::project_semantic_dispatch::cost_receipt::Nesting::InPlace,
            );
            Some(receipt)
        } else {
            None
        };
        self.dispatch
            .dispatch_txn
            .borrow_mut()
            .closedness_proofs
            .complete(
                key,
                CompletedProof {
                    verdict,
                    cost: budget - self.budget,
                    receipt,
                    cycle_dependencies,
                    served_at,
                    reuse,
                    replay,
                },
            );
    }

    fn resume(&mut self, frame: Frame<'a>, answer: Answer) -> Flow {
        match frame {
            Frame::Settle(key) => {
                let open = answer.open();
                let walk = self.current();
                walk.in_flight.remove(&key);
                walk.memo.insert(key, open);
                Flow::open(open)
            }
            Frame::AnyOpen {
                children,
                otherwise,
            } => {
                if answer.open() {
                    return Flow::open(true);
                }
                self.any_open(children, otherwise)
            }
            Frame::EndWalk => {
                self.walks.pop();
                Flow::Answer(answer)
            }
            Frame::NotClosed => Flow::open(!answer.verdict().is_closed()),
            Frame::LeaveProof(span) => {
                self.leave_proof(*span, answer.verdict());
                Flow::Answer(answer)
            }
            Frame::Classified => {
                // Not open ⇒ `ProvenClosed`; open with budget remaining ⇒
                // `ProvenOpen`; open with the budget EXHAUSTED ⇒
                // `Unavailable` — the fuse fired mid-walk, so the openness
                // answer may be the fuse's, not a proof (never collapse a
                // budget abort into a proof).
                Flow::verdict(if !answer.open() {
                    ClosednessVerdict::ProvenClosed
                } else if self.budget == 0 {
                    ClosednessVerdict::Unavailable
                } else {
                    ClosednessVerdict::ProvenOpen
                })
            }
            Frame::Conditional {
                data,
                position,
                stage,
            } => self.conditional_operands(data, position, stage, answer.open()),
            Frame::Mapped(operands) => self.mapped_operand_answered(operands, answer.open()),
            Frame::InstantiationArgs {
                node,
                position,
                base,
                args,
                mut bindings,
            } => {
                let judged = args[bindings.len()];
                bindings.push(if answer.open() {
                    KeyDomainBinding::Open
                } else {
                    KeyDomainBinding::ClosedNode(judged)
                });
                self.instantiation_args(node, position, base, args, bindings)
            }
            Frame::CarrierHead { node, position } => {
                if answer.open() {
                    return Flow::open(true);
                }
                self.carrier_head(node, position)
            }
            Frame::Recipes { env, recipes, next } => {
                let verdict = answer.verdict();
                if !verdict.is_closed() {
                    return Flow::verdict(verdict);
                }
                self.recipes(env, recipes, next)
            }
            Frame::ProjectionIndex { env, index } => {
                let verdict = answer.verdict();
                if !verdict.is_closed() {
                    return Flow::verdict(verdict);
                }
                self.escape(&env, &index, OperandPosition::KeyDomain)
            }
            Frame::Defaults(mut defaults) => {
                let param = defaults.next;
                defaults.env.bindings.0[param] = Some(if answer.open() {
                    // POSITION-SENSITIVE per-argument rule: an unfilled
                    // defaulted parameter whose default does not verify
                    // closed (an open default such as `I = A[number]` over
                    // an open earlier parameter, or an undecidable one)
                    // binds OPEN — exactly like an open caller-supplied
                    // argument — and the body recipes decide where that
                    // openness lands. An open binding confined to member
                    // VALUE positions of a fixed-key body keeps the key
                    // domain CLOSED (`Pick<S<A-open>, K>` over
                    // `type S<A, I = A[number]> = { item?: Fn<I>; … }` still
                    // enumerates path-precisely); a key-reachable use still
                    // opens it through `ParamRef` / the escape walks.
                    // Refusing the whole instantiation here was the
                    // ContentSearch/DropdownMenuContent zero-member
                    // collapse: a value-only defaulted param vetoed a
                    // provably-fixed key set.
                    KeyDomainBinding::Open
                } else {
                    // A VERIFIED-CLOSED default RE-BINDS its parameter to
                    // the lowered default NODE, so a conditional check over
                    // a defaulted parameter (`<T, Use = true> … Use extends
                    // true ? …`) can select through the shared oracle
                    // instead of deferring into an open losing branch.
                    KeyDomainBinding::ClosedNode(defaults.default_node)
                });
                defaults.next += 1;
                self.defaults(defaults)
            }
        }
    }

    /// Open as soon as one of `children` is (in order); `otherwise` when
    /// none is.
    fn any_open(
        &mut self,
        mut children: std::vec::IntoIter<(SemanticNodeId, OperandPosition)>,
        otherwise: bool,
    ) -> Flow {
        let Some((node, position)) = children.next() else {
            return Flow::open(otherwise);
        };
        self.frames.push(Frame::AnyOpen {
            children,
            otherwise,
        });
        Flow::visit(node, position)
    }

    /// Visit `node` at `position` under the current walk: its memoized
    /// verdict, the closed answer of a back edge, or a fresh step whose
    /// verdict settles into the memo.
    ///
    /// OPEN ⇒ reaches an unsubstituted (non-bound) `TypeParam`, an
    /// `Opaque` (resolution miss / degraded), a conditional whose
    /// inspected operands are open, an `IndexedAccess`/`KeyOf`/`Mapped`
    /// over an open operand, an instantiation whose produced KEY SET
    /// depends on an open type argument (an open arg confined to member
    /// VALUE positions of a fixed-key body does not open the domain),
    /// an under-applied / arity-unsatisfied / open-bodied
    /// `InstantiationRef`, an unresolved or open-bodied `DeclRef` alias
    /// chain, or exhausts the budget. CLOSED ⇒ a finite surface proven
    /// without crossing an open node. `Infer` is a conditional-inference
    /// binding placeholder, NOT an unbound generic.
    fn visit(&mut self, node: SemanticNodeId, position: OperandPosition) -> Flow {
        let walk = self.current();
        let key = OpenMemoKey {
            node,
            position,
            role: walk.role,
        };
        if let Some(&open) = walk.memo.get(&key) {
            return Flow::open(open);
        }
        if !walk.in_flight.insert(key) {
            // A back edge contributes no new openness signal and is never
            // memoized before the active node obtains its real verdict.
            return Flow::open(false);
        }
        self.frames.push(Frame::Settle(key));
        self.step(node, position)
    }

    /// The single-node step behind [`Self::visit`]'s memoization.
    fn step(&mut self, node: SemanticNodeId, position: OperandPosition) -> Flow {
        if !self.charge() {
            // Budget exhausted. The KEY-DOMAIN question is conservatively
            // undecidable ⇒ open; the outer-generic-reachability question
            // asks "reaches an unbound outer generic" — not finding one
            // within budget answers that question NO (closed), and the armed
            // runaway fuse remains the backstop. Over-firing the
            // carrier-stop on a deep CLOSED value would be the worse
            // failure.
            return Flow::open(self.undecidable());
        }
        let Some(data) =
            crate::project_semantic_dispatch::node_data_for(self.dispatch.graph(), node)
        else {
            // Unresolved / un-interned node: the KEY-DOMAIN question treats
            // it as open (not provably finite); the
            // outer-generic-reachability question treats it as closed (a
            // missing node carries no outer generic).
            return Flow::open(self.undecidable());
        };
        let role = self.current().role;
        let descend = role.descend_value_surfaces() || position == OperandPosition::ValueSensitive;
        let mut children = Vec::new();
        match data.as_ref() {
            SemanticNodeData::Alias(target) => children.push((*target, position)),
            SemanticNodeData::ClassExpressionInstance { surface, .. } => {
                children.push((*surface, position))
            }
            SemanticNodeData::KeyOf { base } => children.push((*base, OperandPosition::KeyDomain)),
            SemanticNodeData::IndexedAccess { object, index } => {
                children.push((*object, OperandPosition::ValueSensitive));
                if let crate::semantic_query::IndexKey::Computed(node) = index {
                    children.push((*node, OperandPosition::KeyDomain));
                }
            }
            SemanticNodeData::TemplateLiteral { expressions, .. } => {
                children.extend(expressions.iter().map(|node| (*node, position)))
            }
            SemanticNodeData::Object(view) => {
                children.extend(
                    view.index_signatures
                        .iter()
                        .map(|signature| (signature.key_type, position)),
                );
                if descend {
                    children.extend(
                        view.positive_members()
                            .iter()
                            .map(|member| (member.value, position)),
                    );
                    children.extend(view.call_signatures.iter().map(|node| (*node, position)));
                    children.extend(
                        view.construct_signatures
                            .iter()
                            .map(|node| (*node, position)),
                    );
                    children.extend(
                        view.index_signatures
                            .iter()
                            .map(|signature| (signature.value_type, position)),
                    );
                }
            }
            SemanticNodeData::Signature {
                params,
                return_type,
                predicate,
                ..
            } => {
                if descend {
                    children.extend(params.iter().map(|param| (param.ty, position)));
                    children.push((*return_type, position));
                    children.extend(
                        predicate
                            .as_ref()
                            .and_then(|predicate| predicate.ty)
                            .map(|node| (node, position)),
                    );
                }
            }
            SemanticNodeData::Array { element, .. } => {
                if descend {
                    children.push((*element, position));
                }
            }
            SemanticNodeData::Tuple { elements, .. } => {
                children.extend(
                    elements
                        .iter()
                        .filter(|element| element.rest)
                        .map(|element| (element.value, OperandPosition::KeyDomain)),
                );
                if descend {
                    children.extend(elements.iter().map(|element| (element.value, position)));
                }
            }
            composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                children.extend(
                    composite
                        .composite_members()
                        .expect("composite arm")
                        .iter()
                        .map(|node| (*node, position)),
                )
            }
            SemanticNodeData::MergedDecl { contributors } => {
                children.extend(contributors.iter().map(|node| (*node, position)))
            }
            _ => return self.leaf(node, position, data),
        }
        self.any_open(children.into_iter(), false)
    }

    /// Declaration, operator and scoped-binder leaves of the walk.
    fn leaf(
        &mut self,
        node: SemanticNodeId,
        position: OperandPosition,
        data: crate::semantic_query_memo::NodeData,
    ) -> Flow {
        let question = self.current().role.question();
        match data.as_ref() {
            // --- type parameter: open unless BOUND for this walk ---
            // An unsubstituted outer `TypeParam` is an unbound generic;
            // the bound mapper binder `K` (added to `bound_params` by the
            // value-body policy) is a closed local and does NOT open.
            SemanticNodeData::TypeParam { .. } => {
                Flow::open(!self.current().bound_params.contains(&node))
            }
            // An unreduced operation is not PROVABLY closed: its value depends
            // on an operand this walk has not settled.
            SemanticNodeData::IntrinsicApplication { .. } => {
                Flow::open(question.undecidable_is_open())
            }

            // A `DeclPlaceholder` is a RESOLVED-but-deferred declaration
            // reference (canonical + name) — semantically identical to a
            // `DeclRef` for openness, NOT a resolution miss. For the
            // KEY-DOMAIN question its closedness is decided by the bounded
            // prepared-decl transparent-alias-chain proof (a
            // not-provably-closed source key space is open). For the
            // outer-generic-reachability question a declaration reference
            // CANNOT carry the mapper's outer generic, so it does NOT open
            // the value.
            SemanticNodeData::Opaque(crate::semantic_query::QueryError::DeclPlaceholder {
                canonical_id,
                owner,
                name,
                ..
            }) => {
                if matches!(question, OpenQuestion::OuterGenericReachability) {
                    return Flow::open(false);
                }
                // The proof charges the evaluation's ONE budget: a carrier
                // source that resolves through this decl hop back into the
                // walk (e.g. `type Self = { [K in keyof import("./self").Self]: V }`
                // via the `ImportType` bridge) consumes the same budget, so
                // the round-trip terminates bounded.
                self.frames.push(Frame::NotClosed);
                Flow::Ask(Ask::Decl {
                    canonical_id: canonical_id.clone(),
                    owner: *owner,
                    name: name.clone(),
                })
            }
            // A genuine resolution miss / degraded `Opaque` makes the KEY
            // space undecidable (key-domain question ⇒ open) but carries no
            // outer generic (outer-generic-reachability question ⇒ closed).
            SemanticNodeData::Opaque(_) => Flow::open(question.undecidable_is_open()),

            // `Infer` is a conditional-inference BINDING placeholder
            // (`extends UIMessage<infer M, …>`), NOT an unbound generic: a
            // conditional the query selects a branch of reaches here with
            // its bindings substituted, and a placeholder that stays is
            // closed.
            SemanticNodeData::Infer { .. } | SemanticNodeData::InferRef { .. } => Flow::open(false),

            // --- operator shapes ---
            // A conditional is read through the conditional query, the one
            // owner of its decision: the type it reduces to IS its surface —
            // classify only that (an open losing branch is dead and must not
            // false-OPEN the domain). A conditional the query keeps whole
            // classifies the check/extends OPERANDS value-sensitively
            // (branch selection depends on operand VALUES — any open
            // instantiation argument opens them) plus BOTH branches under
            // the surrounding position.
            SemanticNodeData::Conditional { .. } => self.conditional(node, position, data),
            // A nested mapped type splits BY ROLE per operand. Its `source`
            // / mapper `key_space` are KEY-PRODUCTION proof operands
            // ([`OpenRole::KeyDomainProof`]); its `as`-clause `name_remap` is
            // the K-only transform role ([`OpenRole::MappedNameRemap`]); its
            // VALUE body keeps the SURROUNDING role. All key-production
            // operands are walked PINNED at `KeyDomain` regardless of the
            // surrounding position (a value-sensitive parent consumes the
            // mapped type's VALUES, but which KEYS the mapper produces is
            // still a key-domain question: `{ [K in Keys<T>]: string }['a']`
            // over a fixed-key `Keys<T>` must not false-OPEN). The value body
            // is consumed exactly when the surrounding policy consumes values
            // — a `ValueSensitive` operand position or a value-body-
            // descending walk — so `{ [K in 'a']: T }['a']` IS the open `T`
            // and opens. The nested mapper's OWN binder is BOUND for every
            // inspection: a remap `` as `on${Capitalize<K>}` `` over a finite
            // key space is a K-only transform (decidable per key under the
            // remap role), not an open interpolant; a remap reaching an outer
            // `T` stays open. The binder + role scope is local to each
            // operand, hence a walk scope per role (one shared budget, fresh
            // memo).
            SemanticNodeData::Mapped { source, mapper } => {
                let walk = self.current();
                let value = (walk.role.descend_value_surfaces()
                    || position == OperandPosition::ValueSensitive)
                    .then_some((walk.role, position));
                let operands =
                    MappedOperands::new(walk.bound_params.clone(), *source, mapper, value);
                self.mapped_operands(operands)
            }

            // An instantiation's openness depends on the walk's question.
            // The KEY-DOMAIN walks (`Pick`/`Omit` enumeration domain AND
            // the mapped source / key space) judge per-argument: an open
            // type argument does NOT by itself open the produced KEY SET —
            // `Foo<T>` over a decl whose body has a FIXED set of member
            // NAMES (T confined to member VALUE positions) keeps a CLOSED
            // key domain, so `Omit<Foo<T>, …>` and
            // `{ [K in keyof Foo<T>]: V }` still enumerate path-precisely.
            // The per-argument openness vector feeds the instantiation
            // proof (decl exists, arity/defaults satisfiable, prepared
            // body's KEY SET closed under those bindings — an open-bound
            // param opens it only where the body places it in a
            // key-reachable position). The value-body walk asks only "does
            // an ARGUMENT reach the outer generic" — an intrinsic / helper
            // instantiation over the BOUND binder `K` (`Capitalize<K>`,
            // `NonNullable<X[K]>`) does NOT propagate the outer generic and
            // is CLOSED regardless of whether its target body is provably
            // closed; only `Foo<T>` / `Base<T>` (an outer-generic argument)
            // opens it.
            SemanticNodeData::InstantiationRef { base, args } => {
                // Every argument is judged (no short circuit) — the memo
                // keeps a hash-consed repeated open node truthful on
                // revisit, and a closed argument keeps its NODE identity
                // (the binding the conditional branch-selection oracle
                // resolves bound-parameter operands through).
                let args: Arc<[SemanticNodeId]> = args.iter().copied().collect();
                let base = base.clone();
                self.instantiation_args(node, position, base, args, Vec::new())
            }

            SemanticNodeData::DeclRef { identity } => {
                // For the outer-generic-reachability question a declaration
                // reference cannot carry the mapper's outer generic
                // (declarations have their own type parameters), so a bare
                // `DeclRef` is CLOSED — an outer generic reaches the value
                // only as an instantiation ARGUMENT (`Foo<T>`), handled by
                // `InstantiationRef`. For the KEY-DOMAIN question a resolved
                // declaration whose BOUNDED fact-native evaluation proves a
                // finite key domain (the recipe fact under an empty binding
                // environment) is CLOSED; anything else (a free type
                // parameter, an unresolved name, a cycle refusal, or budget
                // exhaustion) is open.
                if matches!(question, OpenQuestion::OuterGenericReachability) {
                    return Flow::open(false);
                }
                // The proof charges the evaluation's ONE budget: a zero-arg
                // `DeclRef` carrier source that resolves back into the walk
                // through this hop (the `ImportType` bridge round-trip)
                // consumes the same budget, so the recursion terminates
                // bounded.
                self.frames.push(Frame::NotClosed);
                Flow::Ask(Ask::Decl {
                    canonical_id: identity.canonical_id.clone(),
                    owner: identity.owner,
                    name: identity.decl_name.clone(),
                })
            }

            SemanticNodeData::Primitive(_)
            | SemanticNodeData::Literal(_)
            | SemanticNodeData::EnumLiteral(_) => Flow::open(false),

            // --- unresolved / fidelity carriers ---
            // A `typeof X` value-rooted lookup is NOT a resolvable
            // type-position declaration head for this purpose (its root is a
            // VALUE namespace, not a type declaration whose key domain we can
            // classify): OPEN if any applied type argument reaches the outer
            // generic — scanned through the shared carrier-arg accessor so the
            // outer-generic-reachability question does NOT false-close `typeof
            // make<T>` over an open `T`. With NO open argument the carrier
            // itself holds no outer generic, so it stays undecidable (open for
            // the key-domain question, closed for the outer-generic question).
            SemanticNodeData::TypeOf(_) => {
                let args: Vec<_> = data
                    .carrier_type_args()
                    .iter()
                    .map(|arg| (*arg, position))
                    .collect();
                self.any_open(args.into_iter(), question.undecidable_is_open())
            }
            // A terminal nominal carrier holds NO outer generic and its key
            // domain is the one declared symbol — closed for both questions.
            SemanticNodeData::TypeOfNominal(_) => Flow::open(false),
            // A `BareRef` / `ImportType` carrier names an UNRESOLVED type-position
            // reference head (`Foo` / `Foo<Arg>` / `import("m").G<Arg>`). The
            // closed-vs-open decision must consult the UNDERLYING declaration's
            // key domain, not just the carrier's applied type arguments:
            //
            //   1. FAST CHECK (policy-scoped) — if any applied type argument is
            //      open AND this walk's policy treats an open argument as
            //      opening, the carrier is OPEN without resolving
            //      (`BareRef<Foo<OpenArg>>` stays open). This holds for the
            //      outer-generic-reachability and the value-sensitive policies
            //      (any open argument opens). It does NOT hold for the
            //      per-argument KEY-DOMAIN policy at a key-domain position: there
            //      an open argument CONFINED to member VALUE positions of a
            //      fixed-key body keeps the key domain CLOSED (the
            //      instantiation proof). Short-cutting OPEN on the open arg
            //      would re-introduce the silent-slot-loss
            //      (`{ [K in keyof Foo<T>]: V }` with `T` value-position-only),
            //      so that case SKIPS the fast check and resolves — the resolved
            //      `InstantiationRef` arm then applies the per-argument rule.
            //   2. RESOLVE the head through the ONE shared self
            //      (`resolve_carrier_subject_node` → `resolve_bare_ref_head` /
            //      `resolve_import_type_head`) under a non-publication
            //      `StructuralTransit` context — NOT a second resolver, NOT a
            //      walker-local resolver, no string heuristics. The resolution
            //      re-enters the SAME dispatcher (`self.dispatch`) so its
            //      `instantiate_active` + `carrier_normalizing` cycle-guard state
            //      is shared (a recursive ref terminates bounded).
            //   3. If the head RESOLVED to a different node (a `DeclPlaceholder` /
            //      `DeclRef` / `InstantiationRef` / `Object` / …), VISIT it at
            //      the same position — the resolved arm (the declaration /
            //      instantiation proof) then decides closedness CORRECTLY (a
            //      fixed-key `Foo<T>` with `T` value-position-only stays CLOSED
            //      and enumerates; a genuinely open declaration stays OPEN).
            //   4. If the head did NOT resolve (miss / recursive-ref-guarded /
            //      not head-resolvable), fall back to the question-correct
            //      undecidable answer — OPEN for the key-domain question (the keys
            //      cannot be enumerated), CLOSED for the outer-generic question (no
            //      outer generic). This preserves the L1 carrier-stop on every
            //      undecidable case (including an unresolvable carrier WITH an
            //      open arg under the per-argument policy: it falls here and the
            //      key-domain question answers OPEN).
            SemanticNodeData::ImportType(_) | SemanticNodeData::BareRef(_) => {
                let args: Vec<_> = data
                    .carrier_type_args()
                    .iter()
                    .map(|arg| (*arg, position))
                    .collect();
                drop(data);
                // The per-argument KEY-DOMAIN policy (mapped source / key
                // space, `Pick`/`Omit` enumeration domain) at a key-domain
                // position is the ONE policy where an open argument does not
                // by itself open the produced key set — it must resolve and
                // apply the per-argument closure rule. Every other
                // policy/position treats any open argument as opening, so the
                // fast check is sound.
                let any_open_arg_opens = !(self.current().role.per_argument_key_domain()
                    && position == OperandPosition::KeyDomain);
                if any_open_arg_opens && !args.is_empty() {
                    self.frames.push(Frame::CarrierHead { node, position });
                    return self.any_open(args.into_iter(), false);
                }
                self.carrier_head(node, position)
            }
            // An unresolved raw-fallback carrier holds no type arguments and no
            // outer generic (closed for the outer-generic question) but is
            // undecidable for the key-domain question.
            SemanticNodeData::ObjectSpreadProgram(_) => Flow::open(true),
            SemanticNodeData::RawFallback { .. } => Flow::open(question.undecidable_is_open()),
            // A synthetic slot-binding is a concrete shallow terminal.
            SemanticNodeData::SyntheticBinding { .. } => Flow::open(false),
            // The sealed callable carrier is a concrete indexed callable
            // position — closed for both open-ness questions.
            SemanticNodeData::DeferredCallable(_) => Flow::open(false),
            SemanticNodeData::Alias(_)
            | SemanticNodeData::ClassExpressionInstance { .. }
            | SemanticNodeData::KeyOf { .. }
            | SemanticNodeData::IndexedAccess { .. }
            | SemanticNodeData::TemplateLiteral { .. }
            | SemanticNodeData::Object(_)
            | SemanticNodeData::Signature { .. }
            | SemanticNodeData::Array { .. }
            | SemanticNodeData::Tuple { .. }
            | SemanticNodeData::Union(_)
            | SemanticNodeData::Intersection(_)
            | SemanticNodeData::MergedDecl { .. } => {
                unreachable!("structural openness is a disjunction over the node's children")
            }
        }
    }

    /// The conditional arm: the reading the conditional query reduces it
    /// to, or — kept whole — its operands and both branches.
    fn conditional(
        &mut self,
        node: SemanticNodeId,
        position: OperandPosition,
        data: crate::semantic_query_memo::NodeData,
    ) -> Flow {
        let SemanticNodeData::Conditional { check, .. } = data.as_ref() else {
            unreachable!("a conditional's openness")
        };
        let check = *check;
        // Read THROUGH the active dispatcher (NOT a freshly-constructed
        // one): it carries the `instantiate_active` /
        // `carrier_normalizing` cycle-guard state, so a recursive ref in
        // the check/extends operands terminates bounded instead of
        // diverging.
        let dispatch = self.dispatch;
        match dispatch.probe_optional(|| {
            crate::project_semantic_dispatch::conditional_decision::exact_reading(
                dispatch.conditional_outcome(node),
            )
        }) {
            Some(
                crate::project_semantic_dispatch::conditional_decision::ConditionalReading::Reduced(
                    selected,
                ),
            ) => return Flow::visit(selected, position),
            Some(_) => {}
            // An unfinished or aborted decision is the context's, not the
            // conditional's: classified whole here, it is no proof to reuse.
            None => self.context_refusal(),
        }
        self.frames.push(Frame::Conditional {
            data,
            position,
            stage: ConditionalStage::Check,
        });
        Flow::visit(check, OperandPosition::ValueSensitive)
    }

    /// The next operand of a conditional kept whole, after the one at
    /// `stage` answered `open`. The branches are applied only once the
    /// operands before them closed.
    fn conditional_operands(
        &mut self,
        data: crate::semantic_query_memo::NodeData,
        position: OperandPosition,
        stage: ConditionalStage,
        open: bool,
    ) -> Flow {
        if open {
            return Flow::open(true);
        }
        let SemanticNodeData::Conditional {
            extends,
            true_branch_ref,
            false_branch_ref,
            pending,
            ..
        } = data.as_ref()
        else {
            unreachable!("a conditional's operands")
        };
        let (next, next_position, next_stage) = match stage {
            ConditionalStage::Check => (
                *extends,
                OperandPosition::ValueSensitive,
                ConditionalStage::Extends,
            ),
            ConditionalStage::Extends => (
                self.dispatch.apply_conditional_branch_pending(
                    *true_branch_ref,
                    pending.as_deref(),
                    true,
                ),
                position,
                ConditionalStage::TrueBranch,
            ),
            ConditionalStage::TrueBranch => {
                let false_branch = self.dispatch.apply_conditional_branch_pending(
                    *false_branch_ref,
                    pending.as_deref(),
                    false,
                );
                return Flow::visit(false_branch, position);
            }
        };
        self.frames.push(Frame::Conditional {
            data,
            position,
            stage: next_stage,
        });
        Flow::visit(next, next_position)
    }

    /// Begin the operand at `operands.stage`, under that operand's walk.
    fn mapped_operands(&mut self, mut operands: MappedOperands) -> Flow {
        let (scope, node, position) = match operands.stage {
            // The SOURCE and the key SPACE share ONE proof walk.
            MappedStage::Source => (
                Some(OpenWalk::with_bound(
                    OpenRole::KeyDomainProof,
                    operands.bound.clone(),
                )),
                operands.source,
                OperandPosition::KeyDomain,
            ),
            MappedStage::KeySpace => (None, operands.key_space, OperandPosition::KeyDomain),
            MappedStage::NameRemap => {
                let Some(remap) = operands.name_remap else {
                    operands.stage = MappedStage::Value;
                    return self.mapped_operands(operands);
                };
                (
                    Some(OpenWalk::with_bound(
                        OpenRole::MappedNameRemap,
                        operands.bound.clone(),
                    )),
                    remap,
                    OperandPosition::KeyDomain,
                )
            }
            MappedStage::Value => {
                let Some((role, position)) = operands.value else {
                    return Flow::open(false);
                };
                (
                    Some(OpenWalk::with_bound(role, operands.bound.clone())),
                    operands.value_expr,
                    position,
                )
            }
        };
        if let Some(scope) = scope {
            self.walks.push(scope);
        }
        self.frames.push(Frame::Mapped(operands));
        Flow::visit(node, position)
    }

    /// The operand at `operands.stage` answered `open`: end its walk where
    /// the next operand does not share it, and stop at the first open one.
    fn mapped_operand_answered(&mut self, mut operands: MappedOperands, open: bool) -> Flow {
        let next = match operands.stage {
            MappedStage::Source if !open => {
                operands.stage = MappedStage::KeySpace;
                return self.mapped_operands(operands);
            }
            MappedStage::Source | MappedStage::KeySpace => MappedStage::NameRemap,
            MappedStage::NameRemap => MappedStage::Value,
            MappedStage::Value => {
                self.walks.pop();
                return Flow::open(open);
            }
        };
        self.walks.pop();
        if open {
            return Flow::open(true);
        }
        operands.stage = next;
        self.mapped_operands(operands)
    }

    /// Judge the instantiation's next argument at the instantiation's
    /// position, or — every argument judged — the instantiation itself.
    fn instantiation_args(
        &mut self,
        node: SemanticNodeId,
        position: OperandPosition,
        base: DeclIdentity,
        args: Arc<[SemanticNodeId]>,
        bindings: Vec<KeyDomainBinding>,
    ) -> Flow {
        let Some(&arg) = args.get(bindings.len()) else {
            return self.instantiation_judged(node, position, &base, bindings);
        };
        self.frames.push(Frame::InstantiationArgs {
            node,
            position,
            base,
            args,
            bindings,
        });
        Flow::visit(arg, position)
    }

    /// The instantiation arm once its per-argument bindings are known.
    fn instantiation_judged(
        &mut self,
        node: SemanticNodeId,
        position: OperandPosition,
        base: &DeclIdentity,
        bindings: Vec<KeyDomainBinding>,
    ) -> Flow {
        let role = self.current().role;
        let any_open = bindings.iter().any(|binding| binding.is_open());
        if position == OperandPosition::ValueSensitive {
            // A VALUE-SENSITIVE operand position (conditional
            // check/extends, indexed-access object): the enclosing operator
            // consumes this instantiation's VALUES, so the per-argument
            // key-domain rule does not apply — ANY open argument opens it.
            if any_open {
                return Flow::open(true);
            }
            if matches!(role.question(), OpenQuestion::OuterGenericReachability) {
                // The outer-generic-reachability question asks only "does an
                // argument reach the outer generic" — an unresolvable base
                // carries none (the DeclRef rule), so all-closed args stay
                // closed.
                return Flow::open(false);
            }
            // KEY-DOMAIN question: all-closed arguments are a CONCRETE
            // surface only when the base actually resolves (prepared decl /
            // registry builtin); an unresolvable base is undecidable ⇒ open.
            let ctx = self.dispatch.ctx;
            return Flow::open(!instantiation_base_is_resolvable(ctx, base, &mut || {
                self.charge()
            }));
        }
        if !role.per_argument_key_domain() {
            // The outer-generic-reachability question asks only "does an
            // ARGUMENT reach the outer generic".
            return Flow::open(any_open);
        }
        if role.concrete_no_open_arg_is_closed() && !any_open {
            // `as`-REMAP role ([`OpenRole::MappedNameRemap`]) with NO outer
            // generic reaching the instantiation: the produced key set is
            // concrete at build time — the mapped key ENUMERATOR (or the
            // deferred `Mapped` shell on plain unavailability) owns it; no
            // carrier-stop. A `Capitalize<K>` / `MixedVis[K]` remap over the
            // BOUND binder `K` is decidable per key. The source / key-space
            // ([`OpenRole::KeyDomainProof`]) and `Pick`/`Omit`
            // enumeration-domain roles do NOT take this shortcut — they must
            // PROVE finiteness below (fall through to the builtin registry /
            // prepared-decl proof), because a value-producing builtin source
            // (`ReturnType<…>`) makes NO closed-key claim and `& T` opens the
            // key domain: short-cutting it CLOSED would leak the closed-arm
            // keys (the false-closed defect).
            return Flow::open(false);
        }
        if base.canonical_id.as_ref() == "__builtin__" {
            // A `__builtin__` base has NO prepared decl, so the prepared-decl
            // key-domain proof could never prove it closed. Judged by the ONE
            // registry-owned rule (`builtin_utility_key_domain_is_closed`):
            // per-utility OUTPUT-KEY semantics — only the arguments that
            // actually produce output keys are judged (`Record`'s open value
            // arg keeps the domain CLOSED; a value-producing utility makes no
            // closed-key claim) — and a nested closed carrier
            // (`Pick<Pick<{…}, 'a' | 'b'>, 'a'>` or `Pick<Partial<{…}>, 'a'>`)
            // must NOT be judged OPEN.
            if builtin_utility_key_domain_is_closed(base.decl_name.as_ref(), &bindings) {
                return Flow::open(false);
            }
            if builtin_utility_key_domain_is_open(base.decl_name.as_ref(), &bindings) {
                return Flow::open(true);
            }
            // Neither rule decides the domain — for a
            // VALUE-PRODUCING utility (`ReturnType`, `InstanceType`,
            // `Awaited`, the string intrinsics, …) that is not a proof of
            // openness: the produced key set derives from argument VALUE
            // structure the per-argument binding walk never inspected. A
            // heritage clause `extends ReturnType<typeof f>` whose flow
            // return is a closed object surface HAS a finite key domain;
            // judging it open carrier-stops a mapped type over the interface
            // and publishes a zero-member surface for a fully modelled shape.
            // Fall back to a bounded one-hop reduction of the instantiation
            // and classify the REDUCED surface with this same walk. Every
            // earlier conservatism is preserved: a partial / truncated /
            // faulted read, a stable carrier-stop (no progress), and a
            // reduced surface that is itself open all keep the OPEN verdict
            // the registry-only rule produced.
            return self.reduced_instantiation(node);
        }
        self.frames.push(Frame::NotClosed);
        Flow::Ask(Ask::Instantiation {
            base: base.clone(),
            args: bindings,
        })
    }

    /// Bounded ONE-HOP reduction fallback behind the builtin registry rule:
    /// is the instantiation `node` open?
    ///
    /// Resolves a `__builtin__` instantiation the per-utility output-key
    /// rule could not prove closed through a structural source-domain demand
    /// (the residual-`InstantiationRef` `Instantiate` hop: bounded,
    /// fail-closed, dep-signature facts recorded into the active tracer),
    /// then classifies the REDUCED node with this same walk at the
    /// key-domain position. OPEN (not provably closed) on a partial /
    /// truncated / faulted demand, on a no-progress stable carrier-stop
    /// (classifying the SAME node would read this walk's own in-flight
    /// back-edge as closed), and on a reduced surface that is itself open —
    /// exactly the verdict the registry-only rule produced for those cases.
    fn reduced_instantiation(&mut self, node: SemanticNodeId) -> Flow {
        if !self.charge() {
            return Flow::open(true);
        }
        // Demand the utility's shallow structural result, not a projected
        // positive-member surface. A flow-backed signature utility can supply
        // its independently certified exact key domain here, retaining each
        // unfinished value as a deferred member demand. Structural transit
        // instead consumes its whole return, whose value limitations cannot
        // establish whether the keys themselves are complete.
        // A partial presence subset alone cannot prove a closed domain.
        let dispatch = self.dispatch;
        let Some(reduced) = dispatch.probe_optional(|| {
            dispatch
                .normalize_node_for_structural_fact_demand(
                    node,
                    ProjectionReductionContext::published(ProjectionMode::Shallow),
                )
                .into_complete_node()
        }) else {
            // A partial demand (a cycle, a connected limit, a fault, a
            // cancellation) answers OPEN for this context only.
            self.context_refusal();
            return Flow::open(true);
        };
        if reduced == node {
            return Flow::open(true);
        }
        Flow::visit(reduced, OperandPosition::KeyDomain)
    }

    /// Resolve an unresolved carrier's head and judge what it names.
    fn carrier_head(&mut self, node: SemanticNodeId, position: OperandPosition) -> Flow {
        if self.dispatch.carrier_normalizing.borrow().contains(&node) {
            // The carrier is being normalized below this evaluation: its
            // head resolution answers the carrier itself, for this context
            // only.
            self.context_refusal();
        }
        let resolved = self
            .dispatch
            .resolve_carrier_subject_node(node, ProjectionReductionContext::structural_transit());
        if resolved != node {
            return Flow::visit(resolved, position);
        }
        Flow::open(self.undecidable())
    }

    /// The body of a bare declaration's proof, inside its in-flight entry.
    fn decl(
        &mut self,
        canonical_id: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        decl_name: &str,
    ) -> Flow {
        let ctx = self.dispatch.ctx;
        if !self.charge() {
            return Flow::verdict(ClosednessVerdict::Unavailable);
        }
        // Root this cross-file consult (decl fact OR barrel hop below) on the
        // active fact tracer — the closedness verdict depends on this file's
        // content, so an edit here must reject the consuming warm entry.
        observe_closedness_walk_consult(ctx, canonical_id);

        // An enum's type is the finite union of its members' literal types:
        // a closed key domain, which no body recipe describes.
        let is_enum = || {
            ctx.prepared_value_decl_return_only(canonical_id, owner, decl_name)
                .is_some_and(|value| {
                    matches!(
                        value.kind,
                        verter_session_query::declarations::ValueDeclKind::Enum
                    )
                })
        };
        let Some(prepared) = ctx.prepared_type_decl_return_only(canonical_id, owner, decl_name)
        else {
            if is_enum() {
                return Flow::verdict(ClosednessVerdict::ProvenClosed);
            }
            // No local declaration at `(canonical, name)` — the name may be a
            // barrel RE-EXPORT (`export { LinkProps } from './link'`) rather
            // than a declaration in this file. Follow the re-export hop
            // through the shallow export map (cache reads only, no reducer /
            // no execute_read) and prove the resolved source decl, bounded by
            // the same budget + in-flight guard. Without this a `Pick`/`Omit`
            // over a barrel-reexported CLOSED interface is mis-classified
            // undecidable (the prepared decl lives in the source file, not
            // the barrel) and the L1 carrier-stop wrongly fires on a
            // genuinely-closed cross-file source.
            if owner == verter_type_expr::TopLevelOwnerId::ordinary_file() {
                let (resolved, route_facts) =
                    ctx.resolve_imported_type_root_with_facts(canonical_id, decl_name);
                ctx.observe_borrowed_signature(&route_facts);
                if let Some(resolved) = resolved {
                    if resolved.canonical_id.as_ref() != canonical_id
                        || resolved.owner != owner
                        || resolved.symbol_name.as_ref() != decl_name
                    {
                        return Flow::Ask(Ask::Decl {
                            canonical_id: resolved.canonical_id.clone(),
                            owner: resolved.owner,
                            name: resolved.symbol_name.clone(),
                        });
                    }
                }
            }
            return Flow::verdict(ClosednessVerdict::Unavailable);
        };
        // The producer-minted closedness fact replaces the transient authored
        // re-borrow. Absent (seeded state, enum group) or empty ⇒ undecidable.
        let Some(recipes) = prepared
            .key_domain_closedness
            .as_ref()
            .map(|fact| fact.body_recipes.clone())
            .filter(|recipes| !recipes.is_empty())
        else {
            return Flow::verdict(if is_enum() {
                ClosednessVerdict::ProvenClosed
            } else {
                ClosednessVerdict::Unavailable
            });
        };
        let bindings = ParamBindings::free(&prepared);
        let env = Rc::new(RecipeEnv { prepared, bindings });
        self.recipes(env, recipes, 0)
    }

    /// The body of an instantiation's proof, inside its in-flight entry.
    fn instantiation(&mut self, base: DeclIdentity, args: Vec<KeyDomainBinding>) -> Flow {
        let ctx = self.dispatch.ctx;
        let arg_count = args.len();
        // Root the consult of the target decl's file on the active fact tracer
        // — the key-domain verdict depends on the prepared fact's content, so
        // an edit to it must reject the consuming warm entry.
        observe_closedness_walk_consult(ctx, base.canonical_id.as_ref());
        let Some(prepared) = ctx.prepared_type_decl_return_only(
            base.canonical_id.as_ref(),
            base.owner,
            base.decl_name.as_ref(),
        ) else {
            // No local declaration at the base identity — it may be a barrel
            // RE-EXPORT (`export { SelectMenuProps } from './props'`) rather
            // than a declaration in this file. Follow ONE re-export hop
            // through the shallow export map (cache reads only) and retry on
            // the resolved source decl, budget-bounded against re-export
            // cycles. Without this an `Omit<SelectMenuProps<T>, K>` over a
            // barrel-reexported generic wrapper is mis-classified undecidable
            // (the wrapper's prepared decl lives in its source file, not the
            // barrel) and the L1 carrier-stop wrongly fires.
            if !self.charge() {
                return Flow::verdict(ClosednessVerdict::Unavailable);
            }
            if base.owner == verter_type_expr::TopLevelOwnerId::ordinary_file() {
                let (resolved, route_facts) = ctx.resolve_imported_type_root_with_facts(
                    base.canonical_id.as_ref(),
                    base.decl_name.as_ref(),
                );
                ctx.observe_borrowed_signature(&route_facts);
                if let Some(resolved) = resolved.filter(|resolved| {
                    resolved.canonical_id.as_ref() != base.canonical_id.as_ref()
                        || resolved.owner != base.owner
                        || resolved.symbol_name.as_ref() != base.decl_name.as_ref()
                }) {
                    return Flow::Ask(Ask::Instantiation {
                        base: DeclIdentity {
                            canonical_id: resolved.canonical_id.clone(),
                            owner: resolved.owner,
                            whole_hash: crate::semantic_query::HashValue::default(),
                            decl_name: resolved.symbol_name.clone(),
                        },
                        args,
                    });
                }
            }
            // Unresolved target ⇒ undecidable.
            return Flow::verdict(ClosednessVerdict::Unavailable);
        };

        let total_params = prepared.type_parameters.len();
        let required_params = prepared
            .type_parameters
            .iter()
            .filter(|p| p.default.is_none())
            .count();
        // Over- or under-application: arity not satisfiable — PROVEN not
        // closed (decided from the declaration's own parameter facts).
        if arg_count < required_params || arg_count > total_params {
            return Flow::verdict(ClosednessVerdict::ProvenOpen);
        }

        // The producer-minted closedness fact replaces the transient authored
        // re-borrow. Absent (seeded state, enum group) or empty ⇒ undecidable.
        let Some(recipes) = prepared
            .key_domain_closedness
            .as_ref()
            .map(|fact| fact.body_recipes.clone())
            .filter(|recipes| !recipes.is_empty())
        else {
            return Flow::verdict(ClosednessVerdict::Unavailable);
        };

        // Binding environment: every declared type-parameter name is bound —
        // the first `arg_count` to the caller's identity-preserving argument
        // bindings, the rest (unfilled, defaulted) to `ClosedAbstract`
        // pending the default verification.
        let bindings = ParamBindings(
            args.into_iter()
                .map(Some)
                .chain(std::iter::repeat_n(
                    Some(KeyDomainBinding::ClosedAbstract),
                    total_params - arg_count,
                ))
                .collect(),
        );
        self.defaults(Defaults {
            env: RecipeEnv { prepared, bindings },
            recipes,
            next: arg_count,
            default_node: SemanticNodeId(0),
        })
    }

    /// Verify the unfilled defaulted parameters from `defaults.next` on,
    /// then evaluate the body recipes.
    ///
    /// Defaults for unfilled params must close UNDER THE SAME BINDINGS: a
    /// default referencing an EARLIER type parameter (`<T, U = T>`) is
    /// closed when that parameter is bound to a closed arg and open when it
    /// is bound to an open one — the default's authored bound slot derefs +
    /// lowers under the ACCUMULATING environment, so the lowered node
    /// carries the earlier parameters' live identities. Processed in
    /// declaration order so later defaults see earlier defaults'
    /// identities.
    fn defaults(&mut self, mut defaults: Defaults) -> Flow {
        while let Some(param) = defaults.env.prepared.type_parameters.get(defaults.next) {
            let Some(slot) = param.default.clone() else {
                // No default for an unfilled param — guarded by the arity
                // check, but stay conservative.
                return Flow::verdict(ClosednessVerdict::Unavailable);
            };
            if !self.charge() {
                return Flow::verdict(ClosednessVerdict::Unavailable);
            }
            let Some(default_body) = deref_slot_body(self.dispatch.ctx, &slot) else {
                // Undecidable default VALUE — bind the parameter OPEN rather
                // than refusing the whole instantiation: the openness only
                // matters WHERE the body places the parameter.
                defaults.env.bindings.0[defaults.next] = Some(KeyDomainBinding::Open);
                defaults.next += 1;
                continue;
            };
            let lowering = escape_lowering_env(
                self.dispatch,
                &defaults.env.prepared,
                &defaults.env.bindings,
            );
            defaults.default_node = lower_body_under_env(
                self.dispatch,
                &defaults.env.prepared,
                &slot,
                &default_body,
                &lowering.env,
            );
            let node = defaults.default_node;
            self.frames.push(Frame::Defaults(defaults));
            return Flow::Ask(Ask::Walk {
                scope: OpenWalk::with_bound(OpenRole::KeyDomainProof, lowering.bound_params),
                node,
                position: OperandPosition::KeyDomain,
            });
        }
        let Defaults { env, recipes, .. } = defaults;
        self.recipes(Rc::new(env), recipes, 0)
    }

    /// Evaluate `recipes[next..]` in order: the first non-closed verdict is
    /// the answer; all closed ⇒ `ProvenClosed`.
    fn recipes(
        &mut self,
        env: Rc<RecipeEnv>,
        recipes: Arc<[ClosednessRecipe]>,
        next: usize,
    ) -> Flow {
        if next >= recipes.len() {
            return Flow::verdict(ClosednessVerdict::ProvenClosed);
        }
        let list = Arc::clone(&recipes);
        self.frames.push(Frame::Recipes {
            env: Rc::clone(&env),
            recipes,
            next: next + 1,
        });
        self.recipe(env, &list[next])
    }

    /// THE recipe evaluator — shared by the bare prepared-decl route (EMPTY
    /// binding environment) and the instantiated route (declared params
    /// bound to identity-preserving [`KeyDomainBinding`]s). One evaluator,
    /// one set of arm semantics — the two routes cannot diverge on what
    /// closes a key domain.
    ///
    /// The recipe arms are BINDING-INDEPENDENT-SOUND producer facts; the two
    /// binding-dependent leaves resolve against the LIVE environment here,
    /// and the general escape ([`ClosednessRecipe::LowerAndClassify`]) routes
    /// through [`Self::escape`] — deref + shallow-lower the authored
    /// position, then classify the NODE with the node walk
    /// ([`OpenRole::KeyDomainProof`]): the SAME classifier the `Pick`/`Omit`
    /// enumeration domain and the mapped source/keyspace run, so the recipe
    /// route and the node route agree by construction. Each evaluated
    /// recipe charges the budget once; decl-level cycles refuse through the
    /// dispatch-wide in-flight guard.
    fn recipe(&mut self, env: Rc<RecipeEnv>, recipe: &ClosednessRecipe) -> Flow {
        if !self.charge() {
            return Flow::verdict(ClosednessVerdict::Unavailable);
        }
        Flow::verdict(match recipe {
            // Finite leaf surfaces: an enumerable key space / concrete value.
            ClosednessRecipe::ClosedLeaf => ClosednessVerdict::ProvenClosed,
            // An object body's NAMED members fix its key set regardless of
            // their value types; the producer minted this arm only where every
            // index-signature key is a syntactically closed scalar (anything
            // else escaped the whole object to the walker).
            ClosednessRecipe::ObjectClosed => ClosednessVerdict::ProvenClosed,
            // A function/constructor TYPE has no properties: its key domain
            // is the closed empty set, as an inline signature's is.
            ClosednessRecipe::CallableLeaf => ClosednessVerdict::ProvenClosed,
            // Composites close iff every arm closes; the first non-closed
            // verdict (document order) is the fold's verdict.
            ClosednessRecipe::AllArms(arms) => {
                return Flow::Ask(Ask::Recipes {
                    env,
                    recipes: Arc::clone(arms),
                });
            }
            // A first-class type-parameter reference is a LEAF type variable.
            // Bound to a verified-closed arg/default it is closed (it carries
            // no own enumerable key space the outer filter would expand);
            // bound OPEN — or FREE (a bare-referenced generic decl body) — its
            // key space is undecidable-open.
            ClosednessRecipe::ParamRef { name } => match env.binding(name) {
                Some(binding) if !binding.is_open() => ClosednessVerdict::ProvenClosed,
                _ => ClosednessVerdict::ProvenOpen,
            },
            // A bare reference: a bound parameter resolves ONLY through its
            // binding; otherwise it is a transparent alias hop — require the
            // referenced declaration's key domain to close. An unresolvable
            // bare name is UNAVAILABLE (undecidable): treating it as a closed
            // leaf would wrongly prove an open body closed and let the L1
            // carrier-stop materialise an undecidable surface.
            ClosednessRecipe::FollowRefByName { name } => {
                if let Some(binding) = env.binding(name) {
                    return Flow::verdict(if binding.is_open() {
                        ClosednessVerdict::ProvenOpen
                    } else {
                        ClosednessVerdict::ProvenClosed
                    });
                }
                let prepared = &env.prepared;
                match prepared.name_resolution.get(name.as_str()) {
                    // A class's base names no type when it is a value — a class
                    // expression a variable holds, or the value an `extends`
                    // expression (`extends Mixin(Base)`) registers beside the
                    // class. The base is that value's instance, whose keys are
                    // fixed where the class is declared: a class with no type
                    // parameters of its own reaches no generic through it.
                    Some(target)
                        if prepared.kind
                            == verter_session_query::declarations::TypeDeclKind::Class
                            && prepared.type_parameters.is_empty()
                            && self
                                .dispatch
                                .ctx
                                .prepared_type_decl_return_only(
                                    &target.canonical_id,
                                    target.owner,
                                    &target.symbol_name,
                                )
                                .is_none()
                            && self
                                .dispatch
                                .ctx
                                .prepared_value_decl_return_only(
                                    &target.canonical_id,
                                    target.owner,
                                    &target.symbol_name,
                                )
                                .is_some() =>
                    {
                        ClosednessVerdict::ProvenClosed
                    }
                    Some(target) => {
                        return Flow::Ask(Ask::Decl {
                            canonical_id: target.canonical_id.clone(),
                            owner: target.owner,
                            name: target.symbol_name.clone(),
                        });
                    }
                    None => ClosednessVerdict::Unavailable,
                }
            }
            // An indexed access is judged OPERAND-WISE, mirroring the node
            // walk's `IndexedAccess` arm: the OBJECT operand is VALUE-SENSITIVE
            // (`Wrap<T>['a']` IS the member value `a`, so a
            // per-argument-closed-but-any-arg-open object must OPEN the access);
            // the INDEX stays a key/keyspace question. Each operand lowers ALONE
            // (so the shared lowerer cannot execute the access and erase the
            // operand policy) and classifies at its pinned position.
            ClosednessRecipe::ValueProjection { object, index } => {
                self.frames.push(Frame::ProjectionIndex {
                    env: Rc::clone(&env),
                    index: index.clone(),
                });
                return self.escape(&env, object, OperandPosition::ValueSensitive);
            }
            ClosednessRecipe::LowerAndClassify { slot } => {
                return self.escape(&env, slot, OperandPosition::KeyDomain);
            }
            ClosednessRecipe::Unsupported => ClosednessVerdict::Unavailable,
        })
    }

    /// The general recipe ESCAPE: deref the authored body position
    /// LEASE-ONLY (the same locator-deref rail the type-parameter bound
    /// slots use), shallow-lower it under the live binding environment
    /// through the ONE shared lowerer (`shallow_lower_type_expr_with_context`,
    /// structural transit — carrier-preserving, no execution, no member
    /// materialisation), and classify the lowered NODE with a
    /// [`OpenRole::KeyDomainProof`] walk at `position`. The derefed
    /// `TypeExpr` is TRANSIENT lowering INPUT only — every semantic decision
    /// runs on nodes. The walk charges the evaluation's ONE budget, so a
    /// recursive body that resolves through the walk back into a proof
    /// terminates bounded; open WITH the budget exhausted is UNAVAILABLE
    /// (the fuse fired — a refusal, not a proof).
    fn escape(&mut self, env: &RecipeEnv, slot: &TypeBodySlot, position: OperandPosition) -> Flow {
        if !self.charge() {
            return Flow::verdict(ClosednessVerdict::Unavailable);
        }
        let Some(body) = deref_slot_body(self.dispatch.ctx, slot) else {
            return Flow::verdict(ClosednessVerdict::Unavailable);
        };
        let lowering = escape_lowering_env(self.dispatch, &env.prepared, &env.bindings);
        let node = lower_body_under_env(self.dispatch, &env.prepared, slot, &body, &lowering.env);
        self.frames.push(Frame::Classified);
        Flow::Ask(Ask::Walk {
            scope: OpenWalk::with_bound(OpenRole::KeyDomainProof, lowering.bound_params),
            node,
            position,
        })
    }
}

/// Whether an `InstantiationRef` BASE resolves to a known declaration —
/// the node-route mirror of the TypeExpr arm's
/// `name_resolution`/registry gate, consulted by the enumeration-domain
/// walk at a `ValueSensitive` operand with all-closed arguments: an
/// unresolvable base is an undecidable surface, not a concrete one.
/// `__builtin__` bases resolve through the `BuiltinUtility` registry or as a
/// global nominal (`RuntimeNominal`, whose carrier is the resolved type);
/// file bases through the prepared-decl cache, with ONE barrel
/// re-export hop (deeper re-export chains stay conservatively
/// unresolvable ⇒ OPEN, the safe direction). The consult is rooted on
/// the active fact tracer — the verdict depends on the consulted files'
/// content.
fn instantiation_base_is_resolvable<C: crate::resolver_core::ResolverCapabilities>(
    ctx: &dyn crate::resolver_core::ResolverContext<C>,
    base: &crate::semantic_query::DeclIdentity,
    charge: &mut dyn FnMut() -> bool,
) -> bool {
    if base.canonical_id.as_ref() == "__builtin__" {
        // A registry utility, or a global nominal (`Function`, `Date`, …)
        // whose carrier is the resolved type itself.
        return verter_session_query::type_solver::builtin::BuiltinUtility::from_name(
            base.decl_name.as_ref(),
        )
        .is_some()
            || crate::intrinsic_registry::RuntimeNominal::of_builtin_identity(base).is_some();
    }
    if !charge() {
        return false;
    }
    observe_closedness_walk_consult(ctx, base.canonical_id.as_ref());
    if ctx
        .prepared_type_decl_return_only(
            base.canonical_id.as_ref(),
            base.owner,
            base.decl_name.as_ref(),
        )
        .is_some()
    {
        return true;
    }
    if base.owner == verter_type_expr::TopLevelOwnerId::ordinary_file() {
        let (resolved, route_facts) = ctx.resolve_imported_type_root_with_facts(
            base.canonical_id.as_ref(),
            base.decl_name.as_ref(),
        );
        ctx.observe_borrowed_signature(&route_facts);
        if let Some(resolved) = resolved.filter(|resolved| {
            resolved.canonical_id.as_ref() != base.canonical_id.as_ref()
                || resolved.owner != base.owner
                || resolved.symbol_name.as_ref() != base.decl_name.as_ref()
        }) {
            observe_closedness_walk_consult(ctx, resolved.canonical_id.as_ref());
            return ctx
                .prepared_type_decl_return_only(
                    resolved.canonical_id.as_ref(),
                    resolved.owner,
                    resolved.symbol_name.as_ref(),
                )
                .is_some();
        }
    }
    false
}

#[cfg(test)]
mod chain_tests {
    use super::{ClosednessChain, CycleDependencies};

    fn proof(name: &str) -> crate::project_semantic_dispatch::InstantiateIdentity {
        (
            verter_type_expr::OwnedStr::from("/chain.ts"),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            verter_type_expr::OwnedStr::from(name),
        )
    }

    fn positions(dependencies: &CycleDependencies) -> Vec<usize> {
        dependencies.iter().map(|&(position, _)| position).collect()
    }

    #[test]
    fn a_refusal_of_a_lower_proof_binds_every_proof_above_it_to_that_proof() {
        let mut chain = ClosednessChain::default();
        assert!(chain.enter(proof("A")));
        assert!(chain.enter(proof("B")));
        assert!(chain.enter(proof("C")));
        assert!(!chain.enter(proof("A")), "A is in flight: a cycle refusal");
        let c = chain.leave();
        assert_eq!(positions(&c), [0], "C refused A, below it");
        assert!(chain.holds(&c));
        let b = chain.leave();
        assert_eq!(positions(&b), [0], "B ran C, so it depends on A too");
        let a = chain.leave();
        assert!(a.is_empty(), "a refusal of A itself is A's own");
        assert!(
            !chain.holds(&c),
            "with A gone, C's refusal no longer applies"
        );
        assert!(chain.enter(proof("A")));
        assert!(
            !chain.holds(&c),
            "a proof of A entered again is not the one C refused"
        );
    }

    #[test]
    fn a_refusal_of_the_innermost_proof_is_its_own() {
        let mut chain = ClosednessChain::default();
        assert!(chain.enter(proof("A")));
        assert!(chain.enter(proof("B")));
        assert!(!chain.enter(proof("B")));
        assert!(chain.leave().is_empty(), "B refused itself");
        assert!(chain.leave().is_empty());
    }

    #[test]
    fn a_reused_proof_binds_the_proof_reusing_it_to_what_it_depended_on() {
        let mut chain = ClosednessChain::default();
        assert!(chain.enter(proof("A")));
        assert!(chain.enter(proof("B")));
        assert!(!chain.enter(proof("A")));
        let b = chain.leave();
        assert!(chain.enter(proof("C")));
        chain.observe(&b);
        assert_eq!(
            positions(&chain.leave()),
            [0],
            "C reused B, which refused A: C is A's context's too"
        );
    }
}
