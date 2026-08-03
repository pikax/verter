//! The store view's captured resolution root, and the resolve-imports
//! `Resolution` validator arm reading it.
//!
//! Pins three properties:
//!
//! 1. a view validates an admitted resolution witness against the world it
//!    CAPTURED — a witness minted in that world is accepted;
//! 2. a view captured after a resolution-visible change refuses that same
//!    witness, while the earlier view keeps accepting it (the arm reads the
//!    capture, never the engine's live registry);
//! 3. a view with no captured world validates no resolution fact at all.

use std::sync::Arc;

use verter_workspace::{
    ReadSetSignature, ResolutionContext, ResolutionPublication, ResolvePhase, ResolveRequestKind,
};

use crate::resolver_store::HostStoreView;
use crate::types::FileLanguage;
use crate::{HostConfig, UpsertRequest, VerterHost};

const OWNER: &str = "/p/main.ts";
const DEPENDENCY: &str = "/p/dep.ts";
const SPECIFIER: &str = "./dep";

const CONTEXT: ResolutionContext = ResolutionContext {
    phase: ResolvePhase::ProviderGraph,
    kind: ResolveRequestKind::EsmImport,
};

fn upsert_ts(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .unwrap_or_else(|error| panic!("upsert {id} must succeed: {error:?}"));
}

/// One resolution demand's admitted witness. A refusal fails the fixture:
/// a non-admitted attempt carries no signature, which would make every
/// downstream assertion vacuous.
fn admitted_witness(host: &VerterHost) -> (Option<String>, ReadSetSignature) {
    let workspace = host.ws();
    let outcome = workspace.resolve_import_outcome(OWNER, SPECIFIER, CONTEXT);
    let target = outcome.result().map(|result| result.source_id.clone());
    match outcome.into_publication() {
        ResolutionPublication::Admitted(admitted) => (target, admitted.signature().clone()),
        ResolutionPublication::Refused(refusal) => panic!(
            "precondition: resolving {SPECIFIER} must admit a witness; refused {:?}",
            refusal.reason()
        ),
    }
}

fn view(host: &VerterHost) -> HostStoreView {
    host.resolver_store_view_read().into_owned_view()
}

/// Whether EVERY fact of `witness` validates under `view` — the same
/// all-facts rule `ReadSetSignature::validates` applies, routed through the
/// session's per-domain `StoreView` dispatch.
fn view_validates(view: &HostStoreView, witness: &ReadSetSignature) -> bool {
    witness
        .facts
        .iter()
        .all(|fact| crate::resolver_core::StoreView::validates(view, fact))
}

/// Whether ANY fact of `witness` is a resolution-currency fact. Guards the
/// cases against a witness that never exercises the new arm.
fn carries_resolution_facts(witness: &ReadSetSignature) -> bool {
    witness.facts.iter().any(|fact| {
        matches!(
            fact,
            verter_workspace::FactVersionRef::ResolveImports(inner)
                if inner.resolution_fact().is_some()
        )
    })
}

fn miss_fixture() -> (VerterHost, ReadSetSignature) {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_ts(
        &host,
        OWNER,
        "import { value } from './dep'\nexport { value }\n",
    );
    let (target, witness) = admitted_witness(&host);
    assert_eq!(
        target, None,
        "precondition: the dependency must be absent for the first demand"
    );
    assert!(
        carries_resolution_facts(&witness),
        "precondition: the admitted witness must carry resolution-currency facts"
    );
    (host, witness)
}

#[test]
fn a_captured_view_validates_a_witness_minted_in_its_own_world() {
    let (host, witness) = miss_fixture();

    assert!(
        view_validates(&view(&host), &witness),
        "the store view must capture the resolution world the witness was \
         minted in and validate every fact of that witness"
    );

    // Mutation recipe: stop stamping `pre.resolution_world` into the
    // snapshot (or restore the blanket `else { return false }` on the
    // `Resolution` arm). The witness then validates nothing.
}

#[test]
fn a_view_captured_after_the_dependency_appears_refuses_the_earlier_witness() {
    let (host, witness) = miss_fixture();
    let captured_at_miss = view(&host);
    assert!(
        view_validates(&captured_at_miss, &witness),
        "precondition: the pre-appearance view must accept the miss witness"
    );

    upsert_ts(&host, DEPENDENCY, "export const value = 1\n");
    // The appearance enters the resolution world through the resolve path's
    // evidence refresh; the demand is what makes the observed value move.
    let (retargeted, _) = admitted_witness(&host);
    assert_eq!(
        retargeted.as_deref(),
        Some(DEPENDENCY),
        "precondition: the demand after the appearance must resolve positively"
    );

    assert!(
        !view_validates(&view(&host), &witness),
        "a view captured after the appearance must refuse the miss witness \
         whose observed path probe has moved"
    );
    assert!(
        view_validates(&captured_at_miss, &witness),
        "the earlier view answers for the world it captured: it must not \
         re-read the live resolution registry and flip to a refusal"
    );

    // Mutation recipe: validate against `host.ws().capture_resolution_world()`
    // at validation time instead of the snapshot's captured root. The last
    // assertion then fails — the retained view starts reporting the
    // post-appearance versions.
}

#[test]
fn a_view_with_no_captured_world_validates_no_resolution_fact() {
    let (_host, witness) = miss_fixture();
    let uncaptured = HostStoreView::default();

    let resolution_facts: Vec<_> = witness
        .facts
        .iter()
        .filter(|fact| {
            matches!(
                fact,
                verter_workspace::FactVersionRef::ResolveImports(inner)
                    if inner.resolution_fact().is_some()
            )
        })
        .collect();

    assert!(
        resolution_facts
            .iter()
            .all(|fact| !crate::resolver_core::StoreView::validates(&uncaptured, fact)),
        "a view that captured no resolution world must fail closed on every \
         resolution fact rather than accept it optimistically"
    );
}

/// The RETAINED-capture half of the resolution-root contract.
///
/// The two cases above take a FRESH view on each side of the mutation, so
/// they only prove fact-precision between two fresh captures. A
/// `StoreViewManager`-cached view is different: it keeps answering out of
/// a FROZEN `Arc<CapturedResolutionWorld>` for as long as the manager
/// reuses it, and the manager reuses it while the
/// `StoreViewValidationToken` is unchanged. So a resolution-visible
/// mutation that advanced a fact WITHOUT moving a token dimension would
/// leave the cached view serving pre-mutation fact versions — a stale
/// warm hit that no fact-precision argument about two fresh captures
/// covers.
///
/// `WorkspaceAccess::set_exact_resolutions` is exactly such a mutation.
/// It advances the `ExactResolution` fact inside the resolution-world
/// write gate and touches nothing else: no content, project, artifact,
/// load, env, identity or overlay state moves. (The `VerterHost` wrapper
/// of the same name additionally bumps `store_view_epoch`; the workspace
/// API underneath it does not, and it is a production entry point.)
///
/// The fixture therefore ASSERTS the token equality it depends on,
/// dimension by dimension, before asserting the refusal — otherwise a
/// future incidental epoch bump would silently turn this back into the
/// two-fresh-captures case it exists to go beyond.
#[test]
fn a_manager_cached_view_refuses_a_witness_a_fact_advance_invalidated() {
    let (host, witness) = miss_fixture();

    let before = view(&host);
    let token_before = before.validation_token();
    assert!(
        view_validates(&before, &witness),
        "precondition: the pre-mutation view must accept the miss witness"
    );

    // Retarget the owner edge through the workspace's exact table. This
    // advances the `ExactResolution` fact the witness observed.
    let applied = host.ws().set_exact_resolutions(
        OWNER,
        vec![verter_workspace::ExactResolution {
            specifier: SPECIFIER.to_string(),
            phase: CONTEXT.phase,
            kind: CONTEXT.kind,
            resolved_canonical_id: Some(DEPENDENCY.to_string()),
            possible_canonical_ids: vec![DEPENDENCY.to_string()],
        }],
    );
    assert!(
        applied.changed,
        "precondition: the exact retarget must actually change workspace state"
    );

    let after = view(&host);
    let token_after = after.validation_token();

    // The mutation moved the resolution-fact generation and NOTHING else.
    // Stated as a hard precondition: if any other dimension moved, the
    // manager would rebuild for that reason and this case would degenerate
    // into `a_view_captured_after_the_dependency_appears_...` above.
    assert_eq!(
        (
            token_before.store_view_epoch,
            token_before.project_generation,
            token_before.artifact_generation,
            token_before.load_generation,
            token_before.content_generation,
            token_before.env_hash_fold,
            token_before.project_identity,
            token_before.overlay_identity,
        ),
        (
            token_after.store_view_epoch,
            token_after.project_generation,
            token_after.artifact_generation,
            token_after.load_generation,
            token_after.content_generation,
            token_after.env_hash_fold,
            token_after.project_identity,
            token_after.overlay_identity,
        ),
        "precondition: an exact retarget must move NO other token dimension — \
         otherwise this case does not exercise the retained-capture hole. \
         before={token_before:?} after={token_after:?}"
    );
    assert_ne!(
        token_before.resolution_fact_generation, token_after.resolution_fact_generation,
        "precondition: the exact retarget must mint a resolution fact version"
    );

    assert!(
        !view_validates(&after, &witness),
        "the view read after a resolution-fact advance must REFUSE the witness \
         that advance invalidated. Without `resolution_fact_generation` in the \
         validation token every other dimension is equal, so the \
         StoreViewManager hands back its CACHED view — which still holds the \
         pre-mutation captured world and still validates the stale witness. \
         before={token_before:?} after={token_after:?}"
    );

    // Mutation recipe (proves this discriminates): drop
    // `resolution_fact_generation` from `PreBuildTokenInputs::capture`
    // (stamp `0`). The two tokens then compare equal, the manager reuses
    // the cached view, and this assertion fails with the stale accept.
}

/// The counter must move on a real fact advance and NOT on a
/// first-observation baseline fill.
///
/// This is what makes it usable as a view-reuse dimension AND as an
/// external-supersession dimension. World IDENTITY would fail both: a cold
/// compute publishes a replacement world just to record the evidence
/// baseline for every path it observes for the first time, so keying on
/// identity would rebuild the view — and refuse the compute's own
/// promotion — on the builder's own discovery work.
///
/// Both halves of "not on a baseline fill" are exercised, because they are
/// different arms: an edge already observed takes the equal-value early
/// return, while an edge over paths the world has NEVER seen takes the
/// `None` arm that RECORDS a baseline. Asserting only the first would leave
/// the recording arm — the one the argument actually rests on — untouched.
#[test]
fn the_resolution_fact_generation_moves_only_on_a_real_fact_advance() {
    let (host, _witness) = miss_fixture();

    // (a) Equal-value: re-resolving an already-observed edge.
    let before_repeat = host.ws().resolution_fact_generation();
    let _repeat = admitted_witness(&host);
    let after_repeat = host.ws().resolution_fact_generation();
    assert_eq!(
        before_repeat, after_repeat,
        "re-resolving an already-observed edge must mint no fact version — a \
         counter that moved here would rebuild the store view on the \
         builder's own work"
    );

    // (b) The RECORDING arm: a demand whose every path observation is a
    // first observation. `/p/other.ts` and the `./unseen-*` candidate paths
    // have never been probed, so each observed value FILLS an unrecorded
    // baseline rather than agreeing with or conflicting with one.
    const OTHER_OWNER: &str = "/p/other.ts";
    upsert_ts(&host, OTHER_OWNER, "export const other = 1\n");
    let before_discovery = host.ws().resolution_fact_generation();
    let discovery = host
        .ws()
        .resolve_import_outcome(OTHER_OWNER, "./unseen-dep", CONTEXT);
    let discovery_witness = match discovery.into_publication() {
        ResolutionPublication::Admitted(admitted) => admitted.signature().clone(),
        ResolutionPublication::Refused(refusal) => panic!(
            "precondition: the first-observation demand must admit a witness; \
             refused {:?}",
            refusal.reason()
        ),
    };
    assert!(
        carries_resolution_facts(&discovery_witness),
        "precondition: the first-observation demand must record resolution \
         facts — otherwise it observed nothing and proves nothing"
    );
    assert!(
        !discovery_witness.resolution_path_canonical_ids().is_empty(),
        "precondition: it must have observed at least one PATH — the \
         recording arm is reached only through a path observation"
    );
    let after_discovery = host.ws().resolution_fact_generation();
    assert_eq!(
        before_discovery, after_discovery,
        "recording a baseline the world had never seen must mint NO fact \
         version. Minting here would make every cold compute externally \
         supersede itself: it would refuse to promote its own result and \
         split identical concurrent requests across singleflight lanes."
    );

    // A mutation whose observed value actually changes does mint one.
    let applied = host.ws().set_exact_resolutions(
        OWNER,
        vec![verter_workspace::ExactResolution {
            specifier: SPECIFIER.to_string(),
            phase: CONTEXT.phase,
            kind: CONTEXT.kind,
            resolved_canonical_id: Some(DEPENDENCY.to_string()),
            possible_canonical_ids: vec![DEPENDENCY.to_string()],
        }],
    );
    assert!(applied.changed, "precondition: the retarget must apply");
    assert!(
        host.ws().resolution_fact_generation() > after_discovery,
        "a mutation that advances an observed resolution fact must mint a \
         fact version and move the counter"
    );
}

/// **A resolution retarget must externally supersede.**
///
/// The promotion fence (`is_stable`), the singleflight/stability coalescing
/// LANE, and the request-scoped bundle memo's compat token all reduce to
/// [`StoreViewValidationToken::external_supersession_fingerprint`]. An exact
/// retarget moves the resolution-fact generation and NO other dimension —
/// the case above asserts exactly that — so leaving that dimension out of
/// the fold makes all three blind to it at once:
///
/// * `is_stable` promotes a result computed against the pre-retarget world;
/// * two requests straddling the retarget share a lane, and the leader's
///   result is handed to the follower with NO revalidation, breaking the
///   lane's stated "validation-equivalent for the promoted result"
///   invariant;
/// * the bundle memo re-serves pre-retarget resolved edges to the very
///   stability retry that exists to escape them.
#[test]
fn a_resolution_retarget_supersedes_the_fence_and_splits_the_lane() {
    let (host, _witness) = miss_fixture();

    let before = view(&host).validation_token();

    let applied = host.ws().set_exact_resolutions(
        OWNER,
        vec![verter_workspace::ExactResolution {
            specifier: SPECIFIER.to_string(),
            phase: CONTEXT.phase,
            kind: CONTEXT.kind,
            resolved_canonical_id: Some(DEPENDENCY.to_string()),
            possible_canonical_ids: vec![DEPENDENCY.to_string()],
        }],
    );
    assert!(applied.changed, "precondition: the retarget must apply");

    let after = view(&host).validation_token();
    assert_ne!(
        before.resolution_fact_generation, after.resolution_fact_generation,
        "precondition: the retarget must mint a resolution fact version"
    );

    assert!(
        before.externally_superseded_by(&after),
        "a retarget is an EXTERNAL mutation: a result computed against \
         `before` must not be promoted under `after`. before={before:?} \
         after={after:?}"
    );
    assert_ne!(
        before.external_supersession_fingerprint(),
        after.external_supersession_fingerprint(),
        "the supersession fingerprint is the `u64` the request executors \
         compare; equal values here promote a pre-retarget result"
    );
    assert_ne!(
        before.lane_fingerprint(),
        after.lane_fingerprint(),
        "the coalescing lane hands a leader's stable result to followers \
         WITHOUT per-follower revalidation, so two requests straddling a \
         retarget must not share a lane"
    );
}

/// Anti-vacuity control: a compute's OWN discovery must NOT supersede.
///
/// Folding a dimension a cold compute advances by itself into the fence
/// makes every such request refuse to promote its own result and splits
/// identical concurrent requests across lanes. Re-resolving an already
/// observed edge — a first-observation baseline fill mints nothing — must
/// therefore leave the fingerprints alone.
#[test]
fn a_computes_own_resolution_discovery_does_not_supersede() {
    let (host, _witness) = miss_fixture();

    let before = view(&host).validation_token();
    let _repeat = admitted_witness(&host);
    let after = view(&host).validation_token();

    assert_eq!(
        before.resolution_fact_generation, after.resolution_fact_generation,
        "precondition: re-resolving an already-observed edge mints no fact \
         version — without this the assertions below are vacuous"
    );
    assert!(
        !before.externally_superseded_by(&after),
        "a compute's own resolution work is not an external mutation"
    );
    assert_eq!(
        before.lane_fingerprint(),
        after.lane_fingerprint(),
        "two identical concurrent cold requests must share ONE lane"
    );
}
