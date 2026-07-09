//! Discriminating unit tests for the SHARED serve-mode decision — pure over the
//! typed eligibility facts + the warm cache (no engine contact).
//!
//! These are the decision-layer half of the risk-class negatives:
//!  - FAIL-OPEN / eligibility-gates-SHARED: each of the five provenance facts
//!    missing IN TURN forces OWNED; only the all-positive set serves SHARED.
//!  - SPLIT-BRAIN: a reconnect (bumped editor-session generation) mints a FRESH
//!    `EngineIdentity` (never the prior one) and the prior warm entry is
//!    UNREACHABLE under the new generation — no stale `--api` handle reuse.
//!  - URI-IDENTITY: case/relative/`.`-`..` variants of one tsconfig canonicalize
//!    to ONE identity, so the reference component is never falsely partitioned.
//!
//! The live halves (a real relay-shim + tsgo) live in
//! `crates/verter_lsp/tests/shared_provider_live.rs`.

use std::sync::Arc;
use std::time::Duration;

use verter_session::external_ts::{
    resolve_reference_canonical_path, AttachFact, BindingFact, ConfigPathProbe, EditorBindingFact,
    EngineSessionFacts, EngineWarmCache, OwnedReason, OwnedSessionFacts, ProjectBinding,
    ProjectResolution, ProxyFact, ReferenceInput, ServeMode, ServingProvenance, SharedSessionFacts,
    VersionGateFact,
};
use verter_session::external_ts::{EnvDims, ProjectEnvDimsSource};
use verter_session::file_artifact_store::ProjectIdentity;
use verter_type_runtime::protocol::TypeProviderError;

use super::{
    apply_local_sync_commit, decide_shared_serve, promote_synced, require_synced_carrier_content,
    reserve_carrier_capturing, resolve_editor_binding, stable_project_identity, sync_commit,
    synced_content, CarrierSlot, CarrierSyncState, CarrierWireOp, InjectAction, PendingKind,
    SharedModeController, SyncCommit, SyncMutex,
};

fn pid(b: u8) -> ProjectIdentity {
    ProjectIdentity([b; 16])
}

fn env_dims(identity: ProjectIdentity) -> EnvDims {
    EnvDims {
        parse_env_hash: [1u8; 16],
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        project_identity: identity,
    }
}

fn test_binding(identity: ProjectIdentity) -> ProjectResolution {
    ProjectResolution::ProjectBinding(ProjectBinding::new_for_test(
        "/ws",
        "/ws/tsconfig.json",
        "7.0.1-rc",
        env_dims(identity),
        Vec::new(),
    ))
}

fn shared_session(generation: u64) -> SharedSessionFacts {
    SharedSessionFacts::new(EngineSessionFacts {
        observed_version: Arc::<str>::from("7.0.1-rc"),
        wire_pin: 42,
        editor_session_generation: generation,
    })
}

fn owned_session(generation: u64) -> OwnedSessionFacts {
    OwnedSessionFacts::new(EngineSessionFacts {
        observed_version: Arc::<str>::from("7.0.1-rc"),
        wire_pin: 42,
        editor_session_generation: generation,
    })
}

/// The all-positive fact set for the queried project `identity` at `generation`.
struct Positive {
    version_gate: VersionGateFact,
    attach: AttachFact,
    binding: BindingFact,
    proxy: ProxyFact,
    editor_binding: EditorBindingFact,
    identity: ProjectIdentity,
    generation: u64,
}

fn positive(identity: ProjectIdentity, generation: u64) -> Positive {
    Positive {
        version_gate: VersionGateFact::Cleared {
            observed_version: Arc::<str>::from("7.0.1-rc"),
        },
        attach: AttachFact::Live(shared_session(generation)),
        binding: BindingFact::from_resolution(&test_binding(identity)),
        proxy: ProxyFact::Available,
        editor_binding: EditorBindingFact::evaluate(&identity, &identity),
        identity,
        generation,
    }
}

fn decide(p: &Positive, warm: &mut EngineWarmCache) -> ServeMode {
    // The queried project declares NO references, so its closure is itself alone.
    decide_shared_serve(
        p.version_gate.clone(),
        p.attach.clone(),
        p.binding,
        p.proxy,
        p.editor_binding,
        p.identity,
        "/ws",
        Arc::from("/ws/tsconfig.json"),
        &[],
        owned_session(p.generation),
        p.generation,
        p.identity,
        warm,
    )
    .mode()
}

// ── FAIL-OPEN / eligibility-gates-SHARED ──

#[test]
fn all_positive_evidence_serves_shared() {
    let mut warm = EngineWarmCache::new();
    let p = positive(pid(7), 1);
    assert_eq!(
        decide(&p, &mut warm),
        ServeMode::Shared,
        "all-positive provenance evidence must serve SHARED"
    );
}

#[test]
fn each_missing_positive_fact_fails_closed_to_owned() {
    let id = pid(7);

    // 1. Version gate not green.
    {
        let mut warm = EngineWarmCache::new();
        let mut p = positive(id, 1);
        p.version_gate = VersionGateFact::NotGreen;
        assert_eq!(
            decide(&p, &mut warm),
            ServeMode::Owned,
            "a not-green version gate must fail closed to OWNED"
        );
    }
    // 2. Attach not live (also drops the SHARED session candidate).
    {
        let mut warm = EngineWarmCache::new();
        let mut p = positive(id, 1);
        p.attach = AttachFact::NotLive;
        assert_eq!(
            decide(&p, &mut warm),
            ServeMode::Owned,
            "no live attach must fail closed to OWNED"
        );
    }
    // 3. No project binding.
    {
        let mut warm = EngineWarmCache::new();
        let mut p = positive(id, 1);
        p.binding = BindingFact::from_resolution(&ProjectResolution::NoProject);
        assert_eq!(
            decide(&p, &mut warm),
            ServeMode::Owned,
            "no resolved project binding must fail closed to OWNED"
        );
    }
    // 4. Proxy unavailable.
    {
        let mut warm = EngineWarmCache::new();
        let mut p = positive(id, 1);
        p.proxy = ProxyFact::Unavailable;
        assert_eq!(
            decide(&p, &mut warm),
            ServeMode::Owned,
            "an unavailable proxy must fail closed to OWNED"
        );
    }
    // 5. Editor binding mismatch.
    {
        let mut warm = EngineWarmCache::new();
        let mut p = positive(id, 1);
        p.editor_binding = EditorBindingFact::evaluate(&id, &pid(9));
        assert_eq!(
            decide(&p, &mut warm),
            ServeMode::Owned,
            "an editor-binding mismatch must fail closed to OWNED"
        );
    }
}

/// An `Ambiguous`/`SyntheticScratch` resolution is NOT a binding — "tsgo seems to
/// know this file" is not eligibility.
#[test]
fn non_binding_resolution_is_not_bound() {
    use verter_session::external_ts::AmbiguityCause;
    let mut warm = EngineWarmCache::new();
    let id = pid(7);
    let mut p = positive(id, 1);
    p.binding = BindingFact::from_resolution(&ProjectResolution::Ambiguous(
        AmbiguityCause::MultipleOwners,
    ));
    assert_eq!(
        decide(&p, &mut warm),
        ServeMode::Owned,
        "an ambiguous (non-binding) resolution must fail closed to OWNED"
    );
    let mut p = positive(id, 1);
    p.binding = BindingFact::from_resolution(&ProjectResolution::synthetic_scratch("scratch"));
    assert_eq!(
        decide(&p, &mut warm),
        ServeMode::Owned,
        "a synthetic-scratch resolution must fail closed to OWNED"
    );
}

// ── EDITOR-BINDING IDENTITY: the fact is keyed on the resolved PROJECT identity,
//    never a bare workspace-root hash, so eligibility cannot spill across projects
//    under one `rootUri` ──

/// Two DISTINCT configured projects under the SAME `rootUri` produce DISTINCT
/// editor-binding facts — SHARED eligibility established for one project can never
/// spill to a sibling. Keying on the workspace-root hash (the prior behaviour) made
/// these two facts EQUAL; keying on the resolved project identity separates them.
#[test]
fn editor_binding_fact_keys_on_project_identity_not_workspace_root() {
    let workspace = "/ws";
    let matching_witness = Some("file:///ws");
    let project_a = pid(1);
    let project_b = pid(2); // a DISTINCT project under the SAME workspace root

    let (fact_a, bound_a) = resolve_editor_binding(project_a, workspace, matching_witness);
    let (fact_b, bound_b) = resolve_editor_binding(project_b, workspace, matching_witness);

    assert_eq!(fact_a, EditorBindingFact::Matched(project_a));
    assert_eq!(fact_b, EditorBindingFact::Matched(project_b));
    assert_eq!(bound_a, project_a);
    assert_eq!(bound_b, project_b);
    assert_ne!(
        fact_a, fact_b,
        "two DISTINCT projects under the same rootUri must produce DISTINCT editor-binding \
         facts — eligibility from one project must never spill to a sibling"
    );

    // Fail-closed: a witness bound to a DIFFERENT workspace ⇒ Mismatch (never a forged
    // match), and the bound identity is never the forged project identity.
    let (mismatch, mismatch_bound) =
        resolve_editor_binding(project_a, workspace, Some("file:///other-ws"));
    assert_eq!(mismatch, EditorBindingFact::Mismatch);
    assert_ne!(
        mismatch_bound, project_a,
        "a mismatch never forges the project identity"
    );

    // No witness root at all also fails closed.
    let (no_witness, _) = resolve_editor_binding(project_a, workspace, None);
    assert_eq!(no_witness, EditorBindingFact::Mismatch);
}

/// The live controller recomputes the editor-binding evidence for EACH decided
/// carrier binding rather than reusing the first-establishment fact for the whole
/// session. A controller established for project A, then queried for a DIFFERENT
/// project B (same workspace, matching editor-binding witness), must decide over B's
/// OWN editor binding — so the composite warm key carries B's editor-binding identity.
///
/// DISCRIMINATING: the warm cache is primed with project B's CORRECT SHARED entry —
/// the slot keyed on B's editor binding. When the controller decides for B it must
/// recompute the editor binding as B and REUSE that primed slot (`WarmShared`). If it
/// instead reused project A's establishment editor-binding evidence, its warm key would
/// carry A, the primed B-slot would be unreachable, and the decision would cold-miss
/// (`ColdShared`). Everything but the editor-binding identity is held identical between
/// the primed entry and the controller's decide, so the provenance isolates exactly the
/// per-binding recompute.
#[test]
fn controller_recomputes_editor_binding_per_decided_binding() {
    let workspace = "/ws";
    let witness = "file:///ws";
    let ts: Arc<str> = Arc::from("/ws/tsconfig.json");
    let gen = 1u64;
    let project_a = pid(1);
    let project_b = pid(2);

    let version_gate = VersionGateFact::Cleared {
        observed_version: Arc::<str>::from("7.0.1-rc"),
    };

    // A shared warm cache primed with project B's CORRECT SHARED serving entry — the
    // slot the controller MUST key when it recomputes the editor binding for B
    // (editor-binding identity = B). Priming is a cold insert.
    let warm = Arc::new(SyncMutex::new(EngineWarmCache::new()));
    let primed = {
        let mut guard = warm.lock();
        decide_shared_serve(
            version_gate.clone(),
            AttachFact::Live(shared_session(gen)),
            BindingFact::from_resolution(&test_binding(project_b)),
            ProxyFact::Available,
            EditorBindingFact::evaluate(&project_b, &project_b),
            project_b,
            workspace,
            Arc::clone(&ts),
            &[],
            owned_session(gen),
            gen,
            project_b,
            &mut guard,
        )
    };
    assert_eq!(
        primed.serving(),
        ServingProvenance::ColdShared,
        "priming project B's warm entry must be a cold insert (non-vacuity)"
    );

    // The controller's establishment decision (computed once for project A) — the
    // initial state. Computed on a throwaway cache so it does not pollute `warm`.
    let establishment = {
        let mut throwaway = EngineWarmCache::new();
        decide_shared_serve(
            version_gate.clone(),
            AttachFact::Live(shared_session(gen)),
            BindingFact::from_resolution(&test_binding(project_a)),
            ProxyFact::Available,
            EditorBindingFact::evaluate(&project_a, &project_a),
            project_a,
            workspace,
            Arc::clone(&ts),
            &[],
            owned_session(gen),
            gen,
            project_a,
            &mut throwaway,
        )
    };
    assert_eq!(
        establishment.mode(),
        ServeMode::Shared,
        "the controller establishes SHARED for project A (non-vacuity)"
    );

    let controller = SharedModeController {
        version_gate,
        attach: AttachFact::Live(shared_session(gen)),
        proxy: ProxyFact::Available,
        // The STABLE editor-binding evidence retained from establishment for project A
        // (matching workspace root + witness root URI) — NOT a frozen `Matched(A)` fact.
        workspace_root: Arc::from(workspace),
        witness_root_uri: Some(Arc::from(witness)),
        owned_session: owned_session(gen),
        observed_version: Arc::<str>::from("7.0.1-rc"),
        warm_cache: Arc::clone(&warm),
        establishment,
    };

    // Decide for project B: the controller must recompute the editor binding as B and
    // reuse B's primed warm slot. Reusing project A's establishment editor binding keys
    // on A and cold-misses the primed B-slot.
    let decided = controller.decide(
        BindingFact::from_resolution(&test_binding(project_b)),
        project_b,
        Arc::clone(&ts),
        &[],
        gen,
    );
    assert_eq!(
        decided.serving(),
        ServingProvenance::WarmShared,
        "decide for project B must recompute the editor binding for B and reuse B's warm \
         serving slot — reusing project A's establishment editor binding keys on A and misses"
    );
}

// ── REFERENCE-CLOSURE: the serve mode is decided over the whole redirect-ON
//    reference-connected component, never per single tsconfig (the split-brain
//    hazard — one connected TS project graph split across SHARED and OWNED) ──

/// A referencing project whose redirect-ON reference points at a SEPARATE tsconfig
/// (an absent closure member) is decided over the whole reference-connected
/// component: the absent member fails it closed to OWNED/`IncompleteComponent` — it is
/// NEVER served SHARED on its own single-tsconfig eligibility. With the references
/// dropped (`references: &[]`) the same project falls to a lone eligible component and
/// serves SHARED — exactly the split-brain regression this discriminates.
#[test]
fn redirect_reference_to_absent_member_fails_closure_closed_to_owned() {
    let mut warm = EngineWarmCache::new();
    let a = pid(7);
    let p = positive(a, 1);

    // A redirect-ON reference from `/ws` to a SEPARATE `/lib` project — absent from
    // the single-project decision snapshot. Its canonical identity is what the
    // decision's identity source mints for the resolved reference path.
    let reference = ReferenceInput::redirect_on("../lib/tsconfig.json");
    let referenced_canonical =
        resolve_reference_canonical_path("../lib/tsconfig.json", "/ws", &RealpathNone)
            .expect("the redirect-ON reference resolves to a canonical path");
    let b = stable_project_identity(&referenced_canonical);
    assert_ne!(a, b, "the referenced project is a DISTINCT identity");

    let decision = decide_shared_serve(
        p.version_gate.clone(),
        p.attach.clone(),
        p.binding,
        p.proxy,
        p.editor_binding,
        a,
        "/ws",
        Arc::from("/ws/tsconfig.json"),
        std::slice::from_ref(&reference),
        owned_session(1),
        1,
        a,
        &mut warm,
    );

    // The closure covers BOTH A and its (absent) referenced member B — decided as ONE
    // unit, not split — and fails closed to OWNED because B's eligibility is unproven.
    assert_eq!(
        decision.mode(),
        ServeMode::Owned,
        "a redirect-ON reference to an absent closure member must fail the whole closure \
         closed to OWNED — never SHARED per single tsconfig"
    );
    assert_eq!(
        decision.decision().owned_reason(),
        Some(OwnedReason::IncompleteComponent),
        "the absent referenced member makes the component incomplete"
    );
    let members: Vec<_> = decision.decision().members().members().collect();
    assert!(
        members.contains(&a) && members.contains(&b),
        "the decision covers the WHOLE reference-connected component {{A, B}}, not just A; \
         got {members:?}"
    );
    // OWNED is never warmed — the no-poison rail.
    assert_eq!(warm.len(), 0, "an OWNED closure decision warms nothing");
}

// ── SPLIT-BRAIN: reconnect mints a fresh identity; the prior warm entry is
//    unreachable under the new generation ──

#[test]
fn reconnect_mints_fresh_identity_and_prior_warm_entry_is_unreachable() {
    let id = pid(7);
    let mut warm = EngineWarmCache::new();

    // A reference-free single-project decision at `(attach_gen, cfg_gen)` — the
    // queried project declares no references, so its closure is itself alone.
    let decide_at = |attach_gen: u64, cfg_gen: u64, warm: &mut EngineWarmCache| {
        decide_shared_serve(
            VersionGateFact::Cleared {
                observed_version: Arc::<str>::from("7.0.1-rc"),
            },
            AttachFact::Live(shared_session(attach_gen)),
            BindingFact::from_resolution(&test_binding(id)),
            ProxyFact::Available,
            EditorBindingFact::evaluate(&id, &id),
            id,
            "/ws",
            Arc::from("/ws/tsconfig.json"),
            &[],
            owned_session(cfg_gen),
            cfg_gen,
            id,
            warm,
        )
    };

    // Generation 1 — a cold SHARED establishment.
    let d1 = decide_at(1, 1, &mut warm);
    assert_eq!(d1.mode(), ServeMode::Shared);
    assert_eq!(
        d1.serving(),
        ServingProvenance::ColdShared,
        "the first establishment is a cold miss"
    );
    let engine_gen1 = d1.decision().engine().editor_session_generation;
    assert_eq!(warm.len(), 1, "the cold SHARED decision warmed one entry");

    // A repeat under the SAME generation reuses the warm entry (proves the warm
    // path is live — the discriminator for the reconnect miss below).
    let d1_again = decide_at(1, 1, &mut warm);
    assert_eq!(
        d1_again.serving(),
        ServingProvenance::WarmShared,
        "a same-generation repeat reuses the warm SHARED serving state"
    );

    // Generation 2 — a RECONNECT. The bumped editor-session generation mints a
    // FRESH EngineIdentity, so the prior warm entry is UNREACHABLE (a MISS, a
    // fresh cold establishment) — never a stale `--api` handle reuse.
    let d2 = decide_at(2, 2, &mut warm);
    assert_eq!(d2.mode(), ServeMode::Shared);
    assert_eq!(
        d2.serving(),
        ServingProvenance::ColdShared,
        "a reconnect is NEVER a warm reuse — the fresh generation misses the prior key"
    );
    let engine_gen2 = d2.decision().engine().editor_session_generation;
    assert_ne!(
        engine_gen1, engine_gen2,
        "the reconnect must mint a FRESH engine identity (generation), never reuse the prior"
    );
    assert_ne!(
        d1.decision().engine(),
        d2.decision().engine(),
        "the reconnect EngineIdentity must differ from the prior — no stale handle reuse"
    );

    // One closure, one mode: the decision covers exactly the queried single-project
    // component and is served by exactly ONE mode (SHARED), never split.
    assert_eq!(
        d2.decision().members().members().collect::<Vec<_>>(),
        vec![id],
        "the decision covers exactly the queried component"
    );
}

// ── URI-IDENTITY: canonicalization folds path variants to one identity ──

struct RealpathNone;
impl ConfigPathProbe for RealpathNone {
    fn realpath(&self, _canonical: &str) -> Option<String> {
        None
    }
}

#[test]
fn tsconfig_path_variants_canonicalize_to_one_identity() {
    let probe = RealpathNone;
    // A referencing tsconfig directory + three ways of naming the SAME referenced
    // config: a `.`-`..` round-trip, a redundant `./`, and a directory reference
    // (resolved to its `tsconfig.json`). All must collapse to ONE canonical path,
    // so the reference component is never falsely partitioned into three nodes.
    let dir = "/ws/packages/app";
    let direct = resolve_reference_canonical_path("../lib/tsconfig.json", dir, &probe)
        .expect("direct reference resolves");
    let dotted = resolve_reference_canonical_path("./../lib/./tsconfig.json", dir, &probe)
        .expect("`.`/`..` round-trip resolves");
    let dir_ref = resolve_reference_canonical_path("../lib", dir, &probe)
        .expect("directory reference resolves to tsconfig.json");
    assert_eq!(
        direct, dotted,
        "a `.`/`..` round-trip must canonicalize to the same config path"
    );
    assert_eq!(
        direct, dir_ref,
        "a directory reference must resolve to the same tsconfig.json"
    );
    // …and therefore to the SAME identity through the folded identity source.
    let id_a: ProjectIdentity = stable_project_identity(&direct);
    let id_b: ProjectIdentity = stable_project_identity(&dotted);
    let id_c: ProjectIdentity = stable_project_identity(&dir_ref);
    assert_eq!(id_a, id_b);
    assert_eq!(id_a, id_c);
    // Discriminator: a genuinely DIFFERENT config resolves to a DIFFERENT identity.
    let other = resolve_reference_canonical_path("../other/tsconfig.json", dir, &probe)
        .expect("a different reference resolves");
    assert_ne!(
        stable_project_identity(&other),
        id_a,
        "a genuinely different tsconfig must NOT collapse to the same identity"
    );
}

// ── CARRIER INJECTION: reserve-before-await is atomic, so exactly ONE concurrent
//    first-open sends didOpen version 1 (no check-then-await TOCTOU) ──

/// 32 threads race to inject the SAME carrier for the first time: exactly ONE reserves
/// the absent slot (`InjectAction::Open` ⇒ a single `didOpen` version 1); every other
/// observes the reserved `PossiblyOpenUnsynced` shell (`InjectAction::ReconcileThenOpen`,
/// never a second `Open`). A check-then-await inject would let multiple threads both observe
/// "absent" and both send `didOpen` version 1.
#[test]
fn concurrent_first_open_reserves_exactly_one_open() {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let injected = Arc::new(parking_lot::Mutex::new(
        HashMap::<String, CarrierSlot>::new(),
    ));
    let open_count = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();
    for _ in 0..32 {
        let injected = Arc::clone(&injected);
        let open_count = Arc::clone(&open_count);
        handles.push(std::thread::spawn(move || {
            if reserve_carrier_capturing(&injected, "/ws/Foo.vue.tsx") == InjectAction::Open {
                open_count.fetch_add(1, Ordering::SeqCst);
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(
        open_count.load(Ordering::SeqCst),
        1,
        "exactly ONE concurrent first-open must reserve the absent slot and send didOpen \
         version 1; every other observes the reserved shell (ReconcileThenOpen), never a \
         second didOpen"
    );
    assert!(
        injected.lock().contains_key("/ws/Foo.vue.tsx"),
        "the carrier remains reserved after the race"
    );
}

/// The three-way reserve classification over the pre-reservation slot state: a VACANT slot
/// Opens (reserving a non-serveable `PossiblyOpenUnsynced` shell); a `PossiblyOpenUnsynced`
/// shell Reconciles-then-opens (never a second bare `didOpen`, never a `didChange` onto an
/// unconfirmed open); a barrier-SYNCED slot Changes (a refresh), transitioning the slot to the
/// non-serveable `OpenUnsyncedContent` UP FRONT so the in-flight refresh serves nothing.
/// Reservation itself NEVER promotes to `Synced` — the reserved text is not served until its
/// barrier is confirmed accepted ([`promote_synced`]).
#[test]
fn reserve_classifies_vacant_synced_and_unsynced_slots() {
    use std::collections::HashMap;
    let injected = parking_lot::Mutex::new(HashMap::<String, CarrierSlot>::new());
    let carrier = "/ws/A.vue.tsx";

    // VACANT → Open; the slot is now a non-serveable PossiblyOpenUnsynced shell.
    assert_eq!(
        reserve_carrier_capturing(&injected, carrier),
        InjectAction::Open,
        "the first reservation of a vacant carrier is Open"
    );
    assert!(
        matches!(
            injected.lock().get(carrier),
            Some(CarrierSlot::PossiblyOpenUnsynced)
        ),
        "reservation leaves a PossiblyOpenUnsynced shell, never a synced slot"
    );
    assert_eq!(
        synced_content(&injected, carrier, carrier),
        None,
        "a reserved-but-not-yet-synced carrier serves no content"
    );

    // PossiblyOpenUnsynced shell → ReconcileThenOpen (NEVER a second bare Open, NEVER a Change).
    assert_eq!(
        reserve_carrier_capturing(&injected, carrier),
        InjectAction::ReconcileThenOpen,
        "a reservation of a PossiblyOpenUnsynced shell reconciles-then-opens, never a second \
         bare didOpen and never a didChange onto an unconfirmed open"
    );

    // Once barrier-SYNCED → Change (a refresh). The reservation transitions the slot to the
    // non-serveable OpenUnsyncedContent UP FRONT, so the in-flight refresh serves NOTHING until
    // its new barrier confirms (an in-flight didChange must never serve the prior synced text).
    promote_synced(&injected, carrier, Arc::from("v1"));
    assert_eq!(
        reserve_carrier_capturing(&injected, carrier),
        InjectAction::Change,
        "a reservation of a barrier-SYNCED carrier is a Change (a refresh)"
    );
    assert!(
        matches!(
            injected.lock().get(carrier),
            Some(CarrierSlot::OpenUnsyncedContent)
        ),
        "a Change reservation transitions the slot to the non-serveable OpenUnsyncedContent up \
         front (never left Synced during the in-flight refresh)"
    );
    assert_eq!(
        synced_content(&injected, carrier, carrier),
        None,
        "an in-flight refresh serves NOTHING until the new barrier confirms (never the prior v1)"
    );
}

/// The sync-outcome consistency oracle. A barrier SUCCESS promotes; a
/// FIRST-OPEN failure retracts the possibly-open Program file; a `didChange` failure
/// fails closed to the open-but-content-uncertain `OpenUnsyncedContent` state.
/// Discriminator: committing the reserved text to the served index regardless of the barrier
/// outcome (`Promote` always) would leave divergent served state after a failed barrier.
#[test]
fn sync_commit_maps_barrier_outcome_to_consistent_action() {
    assert_eq!(sync_commit(InjectAction::Open, true), SyncCommit::Promote);
    assert_eq!(sync_commit(InjectAction::Change, true), SyncCommit::Promote);
    assert_eq!(
        sync_commit(InjectAction::ReconcileThenOpen, true),
        SyncCommit::Promote,
        "a reconcile-then-open that syncs promotes like any accepted open"
    );
    assert_eq!(
        sync_commit(InjectAction::Open, false),
        SyncCommit::RetractOpen,
        "a first-open barrier failure must RETRACT the possibly-open Program file (no phantom open)"
    );
    assert_eq!(
        sync_commit(InjectAction::ReconcileThenOpen, false),
        SyncCommit::RetractOpen,
        "a reconcile-then-open barrier failure also retracts the possibly-open Program file"
    );
    assert_eq!(
        sync_commit(InjectAction::Change, false),
        SyncCommit::MarkOpenUnsyncedContent,
        "a didChange barrier failure must fail closed to OpenUnsyncedContent (never keep the \
         possibly-stale prior synced content, never the unaccepted new text)"
    );
}

/// Serving reads ONLY the barrier-SYNCED content: a reserved-but-not-yet-synced
/// carrier serves nothing (fail-closed), and only a promoted (accepted) injection
/// becomes servable.
#[test]
fn synced_content_serves_only_synced_state() {
    use std::collections::HashMap;
    let injected = parking_lot::Mutex::new(HashMap::<String, CarrierSlot>::new());
    let carrier = "/ws/A.vue.tsx";
    assert_eq!(
        reserve_carrier_capturing(&injected, carrier),
        InjectAction::Open
    );
    assert_eq!(
        synced_content(&injected, carrier, carrier),
        None,
        "a reserved-but-unsynced carrier serves no content (never the optimistic reservation)"
    );
    promote_synced(&injected, carrier, Arc::from("accepted-v1"));
    assert_eq!(
        synced_content(&injected, carrier, carrier).as_deref(),
        Some("accepted-v1"),
        "once the barrier is confirmed accepted, the synced content is servable"
    );
}

/// An in-flight refresh is NON-SERVEABLE from the moment its `didChange` is dispatched until the
/// barrier confirms. A `didChange(v2)` is dispatched BEFORE its barrier, so during the in-flight
/// window the shared Program may already hold `v2`; serving the prior `Synced { v1 }` then would
/// misposition SHARED diagnostics against a stale basis. The reservation transitions the slot to
/// the non-serveable `OpenUnsyncedContent` UP FRONT (before the await), so `synced_content` yields
/// `None` while the refresh is in flight, and promotes to `Synced { v2 }` only once the barrier
/// accepts the new text.
///
/// Discriminating invariant: a slot is serveable IFF it is `Synced` AND no barrier op is in flight
/// for it — the in-flight refresh must serve NOTHING (never the prior `v1`), then serve `v2` after
/// the barrier succeeds.
#[tokio::test]
async fn inflight_refresh_is_non_serveable_until_barrier_confirms() {
    let state = CarrierSyncState::new();
    let carrier = "/ws/InflightRefresh.vue.tsx";

    // Pre-seed a SYNCED slot (prior content v1) via a completed first open.
    state
        .drive(
            carrier,
            PendingKind::Inject(Arc::from("v1")),
            |_op: CarrierWireOp| async { Ok::<(), TypeProviderError>(()) },
        )
        .await
        .expect("first open ok");
    assert_eq!(
        state.synced_content(carrier, carrier).as_deref(),
        Some("v1"),
        "the prior synced content is present before the refresh"
    );

    // A refresh (didChange) whose barrier PARKS until the observer releases it: the sink signals
    // `entered` once it is inside the Change barrier, then waits on `release`. This holds the
    // refresh IN FLIGHT (reserved, wire-dispatched, not yet confirmed) across the observer's read.
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let sink = {
        let entered = Arc::clone(&entered);
        let release = Arc::clone(&release);
        move |op: CarrierWireOp| {
            let entered = Arc::clone(&entered);
            let release = Arc::clone(&release);
            async move {
                if let CarrierWireOp::Change { .. } = op {
                    entered.notify_one();
                    release.notified().await;
                }
                Ok::<(), TypeProviderError>(())
            }
        }
    };

    let drive_fut = state.drive(carrier, PendingKind::Inject(Arc::from("v2")), sink);
    let observe_fut = async {
        // Block until the refresh is actually in flight (parked on its Change barrier).
        entered.notified().await;
        // DURING the in-flight window: the slot is NON-serveable — never the prior synced v1.
        assert_eq!(
            state.synced_content(carrier, carrier),
            None,
            "an in-flight refresh serves NOTHING — never the prior synced v1 (the shared Program \
             may already hold v2)"
        );
        assert!(
            require_synced_carrier_content(state.synced_content(carrier, carrier)).is_err(),
            "an in-flight refresh fails closed for SHARED diagnostics (never Ok against a stale \
             basis)"
        );
        // Release the parked barrier so it resolves successfully.
        release.notify_one();
    };
    let (drive_res, ()) = tokio::join!(drive_fut, observe_fut);
    drive_res.expect("the refresh barrier succeeds once released");

    // On barrier SUCCESS the slot promotes to the new synced content v2 (serveable).
    assert_eq!(
        state.synced_content(carrier, carrier).as_deref(),
        Some("v2"),
        "a confirmed refresh promotes to the new synced content v2 (serveable)"
    );
    // Negative: a confirmed refresh must NOT leave the non-serveable OpenUnsyncedContent fallback.
    assert!(
        !matches!(
            state.injected.lock().get(carrier),
            Some(CarrierSlot::OpenUnsyncedContent)
        ),
        "a confirmed refresh promotes to Synced, never leaves the non-serveable OpenUnsyncedContent"
    );
}

/// The SHARED diagnostics fail-closed gate: a carrier with no barrier-SYNCED content (a
/// `PossiblyOpenUnsynced` shell or a never-injected carrier — both `synced_content` `None`)
/// must FAIL CLOSED (an `Err` the composite treats as OWNED), NEVER serve an `Ok(empty)`
/// SHARED result positioned against an absent barrier-synced basis. Only genuinely
/// barrier-synced content serves.
///
/// Discriminator: passing the `None` content straight into `position_carrier_diagnostics` would
/// yield an `Ok(empty)` SHARED result with no barrier-synced basis instead of failing closed to
/// OWNED.
#[test]
fn non_synced_carrier_fails_closed_for_shared_diagnostics() {
    use std::collections::HashMap;
    let injected = parking_lot::Mutex::new(HashMap::<String, CarrierSlot>::new());
    let carrier = "/ws/A.vue.tsx";

    // A reserved but unconfirmed first-open leaves a `PossiblyOpenUnsynced` shell → no synced
    // content → the diagnostics gate FAILS CLOSED (`Err`), never `Ok(empty)`.
    assert_eq!(
        reserve_carrier_capturing(&injected, carrier),
        InjectAction::Open
    );
    assert_eq!(synced_content(&injected, carrier, carrier), None);
    assert!(
        require_synced_carrier_content(synced_content(&injected, carrier, carrier)).is_err(),
        "a PossiblyOpenUnsynced carrier must fail closed for SHARED diagnostics (never Ok(empty))"
    );
    // A never-injected carrier (plain `None`) also fails closed.
    assert!(
        require_synced_carrier_content(None).is_err(),
        "a carrier with no barrier-synced content fails closed"
    );

    // Once barrier-SYNCED, the gate passes the exact synced content through (serveable).
    promote_synced(&injected, carrier, Arc::from("v1"));
    let served = require_synced_carrier_content(synced_content(&injected, carrier, carrier))
        .expect("a barrier-synced carrier serves its content");
    assert_eq!(
        &*served, "v1",
        "the gate passes the barrier-synced content through unchanged"
    );
}

/// A `didChange` refresh whose sync barrier FAILS/times out fails CLOSED: it leaves the slot
/// the non-serveable `OpenUnsyncedContent` state (the doc is open but its text is UNCERTAIN —
/// the refresh may have applied before its confirmation was lost). It serves NEITHER the prior
/// synced text (a POSSIBLE mismatch against the Program's actual text) NOR the unaccepted new
/// text. A later reserve then classifies the still-open doc as a `Change` (a FRESH `didChange`,
/// never a close+reopen reconcile).
///
/// Discriminating invariant: a lost-confirmation refresh must be non-serveable — both the
/// failed-barrier `synced_content == None` and the `OpenUnsyncedContent` slot assert the doc's
/// text is now uncertain, so the prior `Synced { v1 }` is never served again.
#[test]
fn didchange_failure_marks_open_unsynced_content_non_serveable() {
    use std::collections::HashMap;
    let injected = parking_lot::Mutex::new(HashMap::<String, CarrierSlot>::new());
    let carrier = "/ws/A.vue.tsx";

    // First open syncs v1.
    assert_eq!(
        reserve_carrier_capturing(&injected, carrier),
        InjectAction::Open
    );
    apply_local_sync_commit(
        &injected,
        carrier,
        Arc::from("v1"),
        sync_commit(InjectAction::Open, true),
    );
    assert_eq!(
        synced_content(&injected, carrier, carrier).as_deref(),
        Some("v1")
    );

    // A didChange for v2 whose barrier FAILS: the slot fails closed to OpenUnsyncedContent and
    // serves NOTHING (never the possibly-stale v1, never the unaccepted v2).
    assert_eq!(
        reserve_carrier_capturing(&injected, carrier),
        InjectAction::Change
    );
    apply_local_sync_commit(
        &injected,
        carrier,
        Arc::from("v2"),
        sync_commit(InjectAction::Change, false),
    );
    assert!(
        matches!(
            injected.lock().get(carrier),
            Some(CarrierSlot::OpenUnsyncedContent)
        ),
        "a failed didChange leaves the non-serveable OpenUnsyncedContent slot, never Synced"
    );
    assert_eq!(
        synced_content(&injected, carrier, carrier),
        None,
        "a failed didChange serves NOTHING — never the possibly-stale prior v1 (a lost-confirmation \
         refresh must not serve the prior synced content), never the unaccepted v2"
    );

    // The still-open doc reserves as a Change (a fresh didChange) on the retry — never a
    // reconcile-then-open close+reopen.
    assert_eq!(
        reserve_carrier_capturing(&injected, carrier),
        InjectAction::Change,
        "an OpenUnsyncedContent slot retries via a FRESH didChange (the doc is open), never a \
         close+reopen reconcile"
    );
}

/// The `drive`-level refresh-failure discriminator: a `didChange` refresh whose barrier FAILS
/// leaves the carrier NON-SERVEABLE (`synced_content` returns `None` — never the prior synced
/// text, mispositioning SHARED diagnostics against a stale basis, and never the unaccepted new
/// text), and a RETRY sends a FRESH `didChange` at the latest text (never a close+reopen
/// reconcile), promoting to the new synced content on success.
///
/// Discriminating invariant: a lost-confirmation refresh must be non-serveable — step 2's
/// `synced_content == None` proves the failed refresh stops serving the prior `v1`, recovered by
/// the `OpenUnsyncedContent` fail-closed state.
#[tokio::test]
async fn didchange_failure_is_non_serveable_then_retry_didchange_promotes() {
    use std::sync::Mutex as StdMutex;

    let state = CarrierSyncState::new();
    let carrier = "/ws/RefreshFail.vue.tsx";

    // 1. First open v1 (barrier ok) → synced.
    state
        .drive(
            carrier,
            PendingKind::Inject(Arc::from("v1")),
            |_op: CarrierWireOp| async { Ok::<(), TypeProviderError>(()) },
        )
        .await
        .expect("first open ok");
    assert_eq!(
        state.synced_content(carrier, carrier).as_deref(),
        Some("v1"),
        "the first open synced v1"
    );

    // 2. A didChange for v2 whose barrier FAILS: the carrier becomes NON-SERVEABLE — it must
    //    NOT keep serving the prior v1 (the shared Program's text is now uncertain).
    let changed = state
        .drive(
            carrier,
            PendingKind::Inject(Arc::from("v2")),
            |op: CarrierWireOp| async move {
                match op {
                    CarrierWireOp::Change { .. } => {
                        Err(TypeProviderError::new("didChange barrier timed out"))
                    }
                    _ => Ok::<(), TypeProviderError>(()),
                }
            },
        )
        .await;
    assert!(
        changed.is_err(),
        "the failed refresh surfaces its error (fail-closed)"
    );
    assert_eq!(
        state.synced_content(carrier, carrier),
        None,
        "a FAILED didChange leaves the carrier NON-SERVEABLE — never the prior v1 (a \
         lost-confirmation refresh must not serve the prior synced content), never the unaccepted v2"
    );

    // 3. A RETRY sends a FRESH didChange with the latest text v3 (never a close+reopen), and on
    //    success promotes to the new synced content.
    let record: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
    {
        let record = Arc::clone(&record);
        state
            .drive(
                carrier,
                PendingKind::Inject(Arc::from("v3")),
                move |op: CarrierWireOp| {
                    let record = Arc::clone(&record);
                    async move {
                        match op {
                            CarrierWireOp::Open { content, .. } => {
                                record.lock().unwrap().push(format!("open:{content}"));
                                Ok::<(), TypeProviderError>(())
                            }
                            CarrierWireOp::Change { content, .. } => {
                                record.lock().unwrap().push(format!("change:{content}"));
                                Ok(())
                            }
                            CarrierWireOp::Close => {
                                record.lock().unwrap().push("close".to_string());
                                Ok(())
                            }
                        }
                    }
                },
            )
            .await
            .expect("the retry refresh succeeds");
    }
    let wire = record.lock().unwrap().clone();
    assert_eq!(
        wire,
        vec!["change:v3".to_string()],
        "the retry sends a FRESH didChange with the latest text — never a close+reopen reconcile \
         (no didClose, no didOpen)"
    );
    assert!(
        !wire
            .iter()
            .any(|op| op == "close" || op.starts_with("open")),
        "the retry never reconciles via close+reopen after a failed refresh"
    );
    assert_eq!(
        state.synced_content(carrier, carrier).as_deref(),
        Some("v3"),
        "the successful retry promotes to the new synced content v3"
    );
}

/// A FIRST-OPEN whose sync barrier FAILS/times out marks the local slot
/// `PossiblyOpenUnsynced` (the caller separately best-effort retracts the possibly-open
/// Program file). The slot is PRESENT but NOT serveable, and a later inject RECONCILES it
/// (ReconcileThenOpen) — never removed (removal would drive a duplicate `didOpen` on retry),
/// never a phantom `Change` for a carrier the Program never accepted.
#[test]
fn open_failure_marks_slot_possibly_open_unsynced_for_reconcile() {
    use std::collections::HashMap;
    let injected = parking_lot::Mutex::new(HashMap::<String, CarrierSlot>::new());
    let carrier = "/ws/A.vue.tsx";

    assert_eq!(
        reserve_carrier_capturing(&injected, carrier),
        InjectAction::Open
    );
    // The first-open barrier FAILS → the RetractOpen commit marks the slot PossiblyOpenUnsynced.
    assert_eq!(
        sync_commit(InjectAction::Open, false),
        SyncCommit::RetractOpen
    );
    apply_local_sync_commit(
        &injected,
        carrier,
        Arc::from("v1"),
        sync_commit(InjectAction::Open, false),
    );
    assert!(
        matches!(
            injected.lock().get(carrier),
            Some(CarrierSlot::PossiblyOpenUnsynced)
        ),
        "a failed first-open leaves a PossiblyOpenUnsynced shell (present, never removed)"
    );
    assert_eq!(
        synced_content(&injected, carrier, carrier),
        None,
        "the shell serves no content (fail-closed)"
    );
    // A later inject reconciles the uncertain open (ReconcileThenOpen) — never a phantom Change
    // and never a bare duplicate Open.
    assert_eq!(
        reserve_carrier_capturing(&injected, carrier),
        InjectAction::ReconcileThenOpen,
        "after a failed first-open the carrier reconciles-then-opens, not a phantom Change or a \
         bare duplicate Open"
    );
}

/// A tiny sanity check that the env-dims-source seam type is available to tests
/// (it feeds the real production resolver). Not a behavioral guard.
#[test]
fn env_dims_source_closure_is_usable() {
    let source = |_uri: &str| env_dims(pid(3));
    let dims = source.env_dims_for("/ws/tsconfig.json");
    assert_eq!(dims.project_identity, pid(3));
}

// ── The per-carrier ORDERED lifecycle state machine ([`CarrierSyncState`]). The
//    fake wire "sink" models the shim CONTROL channel (didOpen/didChange/didClose) so
//    the ordering + coalescing are exercised with NO real relay/engine. ──

/// Concurrent open+change ordering. A `didChange` on a carrier must BLOCK on the
/// per-carrier gate until the FIRST `didOpen`'s barrier completes — never race ahead —
/// and the latest content is served after. This is non-vacuous: without the per-carrier
/// gate the Change wire-sends while the Open barrier is still in flight (a didChange ahead
/// of the didOpen), the ordering the assertions below forbid.
#[tokio::test]
async fn concurrent_open_and_change_orders_open_barrier_before_change() {
    use std::sync::Mutex as StdMutex;

    let state = Arc::new(CarrierSyncState::new());
    let carrier = "/ws/Foo.vue.tsx";
    let record: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
    let open_entered = Arc::new(tokio::sync::Notify::new());
    let release_open = Arc::new(tokio::sync::Notify::new());

    // The fake wire sink: the Open barrier records + signals entry, then BLOCKS until
    // released (modeling an in-flight first-open); Change records immediately.
    let make_sink = || {
        let record = Arc::clone(&record);
        let open_entered = Arc::clone(&open_entered);
        let release_open = Arc::clone(&release_open);
        move |op: CarrierWireOp| {
            let record = Arc::clone(&record);
            let open_entered = Arc::clone(&open_entered);
            let release_open = Arc::clone(&release_open);
            async move {
                match op {
                    CarrierWireOp::Open { content, .. } => {
                        record.lock().unwrap().push(format!("open:{content}"));
                        open_entered.notify_one();
                        release_open.notified().await;
                        Ok::<(), TypeProviderError>(())
                    }
                    CarrierWireOp::Change { content, .. } => {
                        record.lock().unwrap().push(format!("change:{content}"));
                        Ok(())
                    }
                    CarrierWireOp::Close => {
                        record.lock().unwrap().push("retract".to_string());
                        Ok(())
                    }
                }
            }
        }
    };

    // A: the FIRST open (v1) — enters the Open barrier and HOLDS the per-carrier gate.
    let a = {
        let state = Arc::clone(&state);
        let sink = make_sink();
        tokio::spawn(async move {
            state
                .drive(carrier, PendingKind::Inject(Arc::from("v1")), sink)
                .await
        })
    };
    open_entered.notified().await;

    // B: a concurrent CHANGE (v2) — must BLOCK on the per-carrier gate until A's Open
    // barrier completes (no didChange races ahead of the didOpen).
    let b = {
        let state = Arc::clone(&state);
        let sink = make_sink();
        tokio::spawn(async move {
            state
                .drive(carrier, PendingKind::Inject(Arc::from("v2")), sink)
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert_eq!(
        *record.lock().unwrap(),
        vec!["open:v1".to_string()],
        "the Change must BLOCK on the gate until the Open barrier completes — no didChange \
         races ahead of the didOpen"
    );

    // Release A's Open barrier: A commits, then B proceeds with the latest content.
    release_open.notify_one();
    a.await.unwrap().expect("open ok");
    b.await.unwrap().expect("change ok");

    assert_eq!(
        *record.lock().unwrap(),
        vec!["open:v1".to_string(), "change:v2".to_string()],
        "ordered: the Open barrier completes before the Change; latest content served"
    );
    assert_eq!(
        state.synced_content(carrier, carrier).as_deref(),
        Some("v2"),
        "the local view serves the latest committed synced content"
    );
}

/// The exact desync: a first-Open TIMEOUT must not clobber a slot a later injection
/// committed. With serialization, the failed first-open marks its slot PossiblyOpenUnsynced +
/// best-effort retracts, then the queued later injection RECONCILES the uncertain shell (a
/// bounded retract) and re-OPENS the latest content and commits — the stale earlier op never
/// clobbers the committed later state. This is non-vacuous: without the gate the concurrent
/// first-open timeout's retract races the later op's promote and leaves the overlay desynced —
/// the desync the serialization asserted below prevents.
#[tokio::test]
async fn failed_first_open_does_not_drop_a_later_committed_change() {
    use std::sync::Mutex as StdMutex;

    let state = Arc::new(CarrierSyncState::new());
    let carrier = "/ws/Bar.vue.tsx";
    let record: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
    let open_entered = Arc::new(tokio::sync::Notify::new());
    let release_open = Arc::new(tokio::sync::Notify::new());

    // The FIRST open (v1) FAILS its barrier (a timeout); the later re-open (v2) and any
    // Change succeed.
    let make_sink = || {
        let record = Arc::clone(&record);
        let open_entered = Arc::clone(&open_entered);
        let release_open = Arc::clone(&release_open);
        move |op: CarrierWireOp| {
            let record = Arc::clone(&record);
            let open_entered = Arc::clone(&open_entered);
            let release_open = Arc::clone(&release_open);
            async move {
                match op {
                    CarrierWireOp::Open { content, .. } if &*content == "v1" => {
                        record.lock().unwrap().push("open:v1".to_string());
                        open_entered.notify_one();
                        release_open.notified().await;
                        Err(TypeProviderError::new("first-open barrier timed out"))
                    }
                    CarrierWireOp::Open { content, .. } => {
                        record.lock().unwrap().push(format!("open:{content}"));
                        Ok::<(), TypeProviderError>(())
                    }
                    CarrierWireOp::Change { content, .. } => {
                        record.lock().unwrap().push(format!("change:{content}"));
                        Ok(())
                    }
                    CarrierWireOp::Close => {
                        record.lock().unwrap().push("retract".to_string());
                        Ok(())
                    }
                }
            }
        }
    };

    // A: the failing first-open (v1) — enters the barrier and holds the gate.
    let a = {
        let state = Arc::clone(&state);
        let sink = make_sink();
        tokio::spawn(async move {
            state
                .drive(carrier, PendingKind::Inject(Arc::from("v1")), sink)
                .await
        })
    };
    open_entered.notified().await;

    // B: a concurrent CHANGE (v2), blocked on the gate behind A's failing first-open.
    let b = {
        let state = Arc::clone(&state);
        let sink = make_sink();
        tokio::spawn(async move {
            state
                .drive(carrier, PendingKind::Inject(Arc::from("v2")), sink)
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Release A: its first-open barrier FAILS → retract + drop the slot; THEN B runs.
    release_open.notify_one();
    let a_res = a.await.unwrap();
    let b_res = b.await.unwrap();
    assert!(
        a_res.is_err(),
        "the failed first-open surfaces its error (fail-closed)"
    );
    assert!(
        b_res.is_ok(),
        "the later injection commits after the failed first-open"
    );

    // The failed first-open marked its slot PossiblyOpenUnsynced + best-effort retracted; B
    // then RECONCILED the uncertain shell (a bounded retract) and re-OPENED the LATEST content
    // — the failed earlier op never clobbered B's commit, and B never bare-re-didOpened onto an
    // uncertain open.
    assert_eq!(
        *record.lock().unwrap(),
        vec![
            "open:v1".to_string(),
            "retract".to_string(),
            "retract".to_string(),
            "open:v2".to_string()
        ],
        "ordered: the failed first-open marks the slot PossiblyOpenUnsynced + best-effort \
         retracts, then the later op RECONCILES (a bounded retract) before re-opening the latest \
         content — never a bare re-didOpen onto an uncertain open"
    );
    assert_eq!(
        state.synced_content(carrier, carrier).as_deref(),
        Some("v2"),
        "the local view serves the later committed content — the failed first-open did not drop it"
    );
}

/// The coalescing content of a drained [`PendingKind::Inject`] op (panics on a close —
/// used by tests that only queue injections).
fn drained_inject(kind: &PendingKind) -> &str {
    match kind {
        PendingKind::Inject(content) => content,
        PendingKind::Close => panic!("expected a pending Inject op, got Close"),
    }
}

/// The coalescing core: a gate holder drains the NEWEST pending op (not each
/// intermediate edit), skips when the latest is already committed, and never regresses
/// to a stale lower-seq submission. Deterministic over the coalescing primitives.
#[test]
fn coalescing_drains_latest_and_skips_committed() {
    let state = CarrierSyncState::new();
    let carrier = "/ws/Baz.vue.tsx";

    // Three edits queue; the gate holder drains only the LATEST (v3).
    state.record_pending(carrier, 1, PendingKind::Inject(Arc::from("v1")));
    state.record_pending(carrier, 2, PendingKind::Inject(Arc::from("v2")));
    state.record_pending(carrier, 3, PendingKind::Inject(Arc::from("v3")));
    let (seq, kind) = state.take_drainable(carrier).expect("newest is drainable");
    assert_eq!(
        (seq, drained_inject(&kind)),
        (3, "v3"),
        "a burst coalesces to the LATEST content, not each intermediate edit"
    );

    // After committing the latest, there is nothing more to drain (v1/v2 coalesced).
    state.mark_committed(carrier, 3);
    assert!(
        state.take_drainable(carrier).is_none(),
        "the latest being committed ⇒ no redundant re-sync (coalesced away)"
    );

    // A newer submission re-arms the drain.
    state.record_pending(carrier, 4, PendingKind::Inject(Arc::from("v4")));
    let (seq, kind) = state
        .take_drainable(carrier)
        .expect("a newer submission re-arms the drain");
    assert_eq!((seq, drained_inject(&kind)), (4, "v4"));

    // A STALE (lower-seq) submission never regresses the latest.
    state.record_pending(carrier, 2, PendingKind::Inject(Arc::from("stale")));
    let (seq, kind) = state.take_drainable(carrier).expect("still drainable");
    assert_eq!(
        (seq, drained_inject(&kind)),
        (4, "v4"),
        "a stale lower-seq submission never regresses the latest pending content"
    );
}

/// The unified coalescing cell: a close and an injection share ONE pending slot, so the
/// NEWEST op supersedes an older one of either kind — a close supersedes a queued
/// injection (no op after the close) and a newer injection supersedes an older close (a
/// genuine reopen). Because close routes through the SAME pending cell, a queued injection
/// cannot drain and reopen a just-closed carrier.
#[test]
fn close_and_injection_supersede_each_other_by_newest_seq() {
    let state = CarrierSyncState::new();
    let carrier = "/ws/Q.vue.tsx";

    // A queued injection then a later CLOSE: the newest op (the close) wins the drain.
    state.record_pending(carrier, 1, PendingKind::Inject(Arc::from("v1")));
    state.record_pending(carrier, 2, PendingKind::Close);
    let (seq, kind) = state
        .take_drainable(carrier)
        .expect("the close is drainable");
    assert_eq!(seq, 2, "the newest op (the close) wins the coalesced drain");
    assert!(
        matches!(kind, PendingKind::Close),
        "a close SUPERSEDES an older queued injection (no op after the close)"
    );

    // A newer REOPEN (a fresh injection) supersedes the close — a genuine reopen.
    state.record_pending(carrier, 3, PendingKind::Inject(Arc::from("v2")));
    let (seq, kind) = state
        .take_drainable(carrier)
        .expect("the reopen is drainable");
    assert_eq!(seq, 3);
    assert_eq!(
        drained_inject(&kind),
        "v2",
        "a newer injection SUPERSEDES an older close (a genuine reopen)"
    );
}

/// A close is ORDERED through the SAME per-carrier gate as injection: a close that
/// races a still-in-flight first `didOpen` must BLOCK on the gate until the open
/// barrier completes (never a `didClose` interleaved with the in-flight injection),
/// then close the carrier — never reopen it (no op after a committed close).
///
/// This is non-vacuous: without routing close through the gate the close would send
/// `didClose` immediately (bypassing the per-carrier gate), so the wire would record a
/// `didClose` while the `didOpen` barrier is still in flight — the op-around-close ordering
/// violation (leak/reopen of a closed carrier) that the gate ordering asserted below prevents.
#[tokio::test]
async fn close_is_ordered_through_the_gate_behind_an_in_flight_open() {
    use std::sync::Mutex as StdMutex;

    let state = Arc::new(CarrierSyncState::new());
    let carrier = "/ws/Foo.vue.tsx";
    let record: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
    let open_entered = Arc::new(tokio::sync::Notify::new());
    let release_open = Arc::new(tokio::sync::Notify::new());

    // The fake wire sink: the Open barrier records + signals entry, then BLOCKS until
    // released (an in-flight first-open); Change/Close record immediately.
    let make_sink = || {
        let record = Arc::clone(&record);
        let open_entered = Arc::clone(&open_entered);
        let release_open = Arc::clone(&release_open);
        move |op: CarrierWireOp| {
            let record = Arc::clone(&record);
            let open_entered = Arc::clone(&open_entered);
            let release_open = Arc::clone(&release_open);
            async move {
                match op {
                    CarrierWireOp::Open { content, .. } => {
                        record.lock().unwrap().push(format!("open:{content}"));
                        open_entered.notify_one();
                        release_open.notified().await;
                        Ok::<(), TypeProviderError>(())
                    }
                    CarrierWireOp::Change { content, .. } => {
                        record.lock().unwrap().push(format!("change:{content}"));
                        Ok(())
                    }
                    CarrierWireOp::Close => {
                        record.lock().unwrap().push("close".to_string());
                        Ok(())
                    }
                }
            }
        }
    };

    // A: the FIRST open (v1) — enters the Open barrier and HOLDS the per-carrier gate.
    let a = {
        let state = Arc::clone(&state);
        let sink = make_sink();
        tokio::spawn(async move {
            state
                .drive(carrier, PendingKind::Inject(Arc::from("v1")), sink)
                .await
        })
    };
    open_entered.notified().await;

    // B: a concurrent CLOSE — must BLOCK on the per-carrier gate until A's Open barrier
    // completes (no didClose interleaved with the in-flight didOpen).
    let b = {
        let state = Arc::clone(&state);
        let sink = make_sink();
        tokio::spawn(async move { state.drive(carrier, PendingKind::Close, sink).await })
    };
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert_eq!(
        *record.lock().unwrap(),
        vec!["open:v1".to_string()],
        "the close must BLOCK on the gate until the Open barrier completes — no didClose \
         interleaved with the in-flight didOpen"
    );

    // Release A's Open barrier: A commits (slot open + synced), then B closes it.
    release_open.notify_one();
    a.await.unwrap().expect("open ok");
    b.await.unwrap().expect("close ok");

    // The final wire order is open THEN close (ordered), and the carrier is CLOSED —
    // never reopened after the committed close.
    assert_eq!(
        *record.lock().unwrap(),
        vec!["open:v1".to_string(), "close".to_string()],
        "close is ordered AFTER the in-flight open's barrier — didOpen then didClose, no reopen"
    );
    assert!(
        state.synced_content(carrier, carrier).is_none(),
        "after the committed close the carrier slot is gone (no served content, no reopen)"
    );
}

/// A close of a NEVER-opened carrier is a no-op — it sends no `didClose` (there is no
/// Program document to retract), so the gate/coalescing never fabricates a spurious
/// wire op for a carrier that was never injected.
#[tokio::test]
async fn close_of_never_opened_carrier_is_a_noop() {
    use std::sync::Mutex as StdMutex;

    let state = CarrierSyncState::new();
    let carrier = "/ws/Never.vue.tsx";
    let record: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
    let sink = {
        let record = Arc::clone(&record);
        move |op: CarrierWireOp| {
            let record = Arc::clone(&record);
            async move {
                if let CarrierWireOp::Close = op {
                    record.lock().unwrap().push("close".to_string());
                }
                Ok::<(), TypeProviderError>(())
            }
        }
    };

    state
        .drive(carrier, PendingKind::Close, sink)
        .await
        .expect("close ok");
    assert!(
        record.lock().unwrap().is_empty(),
        "closing a never-opened carrier sends NO didClose (nothing to retract)"
    );
}

/// Closing a carrier PRUNES its per-carrier gate + pending state (not just the
/// injected slot), so the per-carrier maps track the CURRENT open set, not the
/// cumulative touched set across a long opt-in session. Without the prune,
/// `gate_for` / `record_pending` would insert entries that close never removed, so the
/// gates/pending maps would grow monotonically across open/close churn.
#[tokio::test]
async fn close_prunes_per_carrier_gate_and_pending_state() {
    let state = CarrierSyncState::new();
    let carrier = "/ws/Churn.vue.tsx";

    // Open the carrier (reserves the slot + creates the gate + pending cell).
    state
        .drive(
            carrier,
            PendingKind::Inject(Arc::from("v1")),
            |_op: CarrierWireOp| async { Ok::<(), TypeProviderError>(()) },
        )
        .await
        .expect("open ok");
    assert!(state.injected.lock().contains_key(carrier));
    assert!(state.gates.lock().contains_key(carrier));
    assert!(state.pending.lock().contains_key(carrier));

    // Close it (through the ordered gate).
    state
        .drive(carrier, PendingKind::Close, |_op: CarrierWireOp| async {
            Ok::<(), TypeProviderError>(())
        })
        .await
        .expect("close ok");

    // After the close the carrier's ENTIRE per-carrier state is pruned — the slot,
    // the gate, AND the pending cell — so the maps do not grow across open/close churn.
    assert!(
        !state.injected.lock().contains_key(carrier),
        "close drops the injected slot"
    );
    assert!(
        !state.gates.lock().contains_key(carrier),
        "close PRUNES the per-carrier gate (the maps track the current open set, not the \
         cumulative touched set)"
    );
    assert!(
        !state.pending.lock().contains_key(carrier),
        "close PRUNES the per-carrier pending cell"
    );
}

/// Race-safety: a close does NOT prune the gate/pending when a NEWER op is already
/// queued for the carrier — the queued op still owns the gate Arc, so pruning it would
/// split ordering onto a fresh Arc. A reopen queued behind an in-flight close keeps the
/// per-carrier state intact and reopens the carrier.
#[tokio::test]
async fn close_does_not_prune_when_a_newer_op_is_queued() {
    use std::sync::Mutex as StdMutex;

    let state = Arc::new(CarrierSyncState::new());
    let carrier = "/ws/Reopen.vue.tsx";
    let record: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
    let close_entered = Arc::new(tokio::sync::Notify::new());
    let release_close = Arc::new(tokio::sync::Notify::new());

    // The close BLOCKS in its wire barrier (holding the gate) until released; Open /
    // Change record immediately.
    let make_sink = || {
        let record = Arc::clone(&record);
        let close_entered = Arc::clone(&close_entered);
        let release_close = Arc::clone(&release_close);
        move |op: CarrierWireOp| {
            let record = Arc::clone(&record);
            let close_entered = Arc::clone(&close_entered);
            let release_close = Arc::clone(&release_close);
            async move {
                match op {
                    CarrierWireOp::Close => {
                        record.lock().unwrap().push("close".to_string());
                        close_entered.notify_one();
                        release_close.notified().await;
                        Ok::<(), TypeProviderError>(())
                    }
                    CarrierWireOp::Open { content, .. } => {
                        record.lock().unwrap().push(format!("open:{content}"));
                        Ok(())
                    }
                    CarrierWireOp::Change { content, .. } => {
                        record.lock().unwrap().push(format!("change:{content}"));
                        Ok(())
                    }
                }
            }
        }
    };

    // Open + commit first (so the close has an open slot to retract), through the
    // recording sink so `open:v1` is captured.
    state
        .drive(carrier, PendingKind::Inject(Arc::from("v1")), make_sink())
        .await
        .expect("open ok");

    // A: the close — enters the barrier and HOLDS the gate.
    let a = {
        let state = Arc::clone(&state);
        let sink = make_sink();
        tokio::spawn(async move { state.drive(carrier, PendingKind::Close, sink).await })
    };
    close_entered.notified().await;

    // B: a REOPEN queued behind the in-flight close (records its pending op + fetches
    // the gate Arc, then blocks on the gate).
    let b = {
        let state = Arc::clone(&state);
        let sink = make_sink();
        tokio::spawn(async move {
            state
                .drive(carrier, PendingKind::Inject(Arc::from("v2")), sink)
                .await
        })
    };
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Release the close: it must NOT prune the gate/pending because B is queued.
    release_close.notify_one();
    a.await.unwrap().expect("close ok");
    b.await.unwrap().expect("reopen ok");

    // B reopened the carrier — the queued op survived the close (never orphaned onto a
    // fresh gate Arc).
    assert_eq!(
        state.synced_content(carrier, carrier).as_deref(),
        Some("v2"),
        "the reopen queued behind the close committed (the close did not orphan it)"
    );
    assert_eq!(
        *record.lock().unwrap(),
        vec![
            "open:v1".to_string(),
            "close".to_string(),
            "open:v2".to_string()
        ],
        "ordered: open, then close, then the queued reopen"
    );
}

/// A FAILED close (its wire `didClose` returns an `Err` — the shared Program may still hold
/// the doc open) must NOT drop the local slot to VACANT. The close transitions the slot to a
/// non-serveable `PossiblyOpenUnsynced` shell UP FRONT and, on the failure, LEAVES that shell
/// (reconcilable) rather than removing it — and it FAILS CLOSED (returns `Err`), does NOT
/// mark the close committed, and does NOT prune the per-carrier state.
///
/// Discriminating invariant: a failed close must not strand the carrier VACANT. The close
/// transitions the slot to the non-serveable `PossiblyOpenUnsynced` shell before the barrier and
/// LEAVES it on failure, so a later inject reconciles instead of sending a bare duplicate `didOpen`.
#[tokio::test]
async fn failed_close_leaves_reconcilable_shell_and_does_not_prune() {
    let state = CarrierSyncState::new();
    let carrier = "/ws/FailedCloseShell.vue.tsx";

    // Open + commit the carrier (slot present + synced).
    state
        .drive(
            carrier,
            PendingKind::Inject(Arc::from("v1")),
            |_op: CarrierWireOp| async { Ok::<(), TypeProviderError>(()) },
        )
        .await
        .expect("open ok");
    assert_eq!(
        state.synced_content(carrier, carrier).as_deref(),
        Some("v1"),
        "the carrier is synced after the open"
    );

    // A close whose wire `didClose` FAILS (the shared Program may still hold the doc open).
    let closed = state
        .drive(
            carrier,
            PendingKind::Close,
            |op: CarrierWireOp| async move {
                match op {
                    CarrierWireOp::Close => Err(TypeProviderError::new("wedged didClose")),
                    _ => Ok::<(), TypeProviderError>(()),
                }
            },
        )
        .await;

    // Fail closed to OWNED — never a silent `Ok`.
    assert!(
        closed.is_err(),
        "a failed close must fail closed (Err), never a silent Ok"
    );
    // The slot is a reconcilable `PossiblyOpenUnsynced` shell — NOT vacant, NOT served.
    assert!(
        matches!(
            state.injected.lock().get(carrier),
            Some(CarrierSlot::PossiblyOpenUnsynced)
        ),
        "a failed close leaves a PossiblyOpenUnsynced shell (present, reconcilable) — never VACANT"
    );
    assert!(
        state.synced_content(carrier, carrier).is_none(),
        "the shell serves no content after a failed close (fail-closed)"
    );
    // A failed close must NOT prune (it skips mark_committed and leaves a live shell): the
    // per-carrier gate + pending survive for the reconciling retry.
    assert!(
        state.gates.lock().contains_key(carrier),
        "a failed close does NOT prune the gate (the carrier is not idle — the shell is reconcilable)"
    );
    assert!(
        state.pending.lock().contains_key(carrier),
        "a failed close does NOT prune the pending cell"
    );
}

/// The KEY close-path duplicate-`didOpen` discriminator (the symmetric twin of
/// [`super::shared_tests::cancelled_first_open_retry_reconciles_via_close_then_fresh_open`]).
/// After a FAILED close the shared Program's wire state is UNCERTAIN (the `didClose` may not
/// have reached it), so a RETRY inject must RECONCILE — a bounded `didClose` FIRST, THEN a
/// fresh `didOpen` — never a bare duplicate `didOpen` onto a possibly-still-open Program file.
///
/// Discriminating invariant: after a failed close the retry must NOT classify the carrier
/// Vacant → Open and send a SECOND bare `didOpen` (wire `[open, close, open]`, no intervening
/// reconcile `didClose`). The failed close leaves `PossiblyOpenUnsynced`, so the retry reconciles
/// (`[open, close, close, open]`).
#[tokio::test]
async fn failed_close_retry_reconciles_via_close_then_fresh_open() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex as StdMutex;

    let state = CarrierSyncState::new();
    let carrier = "/ws/FailedCloseRetry.vue.tsx";
    let record: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
    let closes = Arc::new(AtomicUsize::new(0));

    // The fake wire sink: opens/changes record + succeed; the FIRST close (the top-level
    // close) records + FAILS (the shared Program may still hold the doc open); every LATER
    // close (the reconcile) records + succeeds.
    let make_sink = || {
        let record = Arc::clone(&record);
        let closes = Arc::clone(&closes);
        move |op: CarrierWireOp| {
            let record = Arc::clone(&record);
            let closes = Arc::clone(&closes);
            async move {
                match op {
                    CarrierWireOp::Open { content, .. } => {
                        record.lock().unwrap().push(format!("open:{content}"));
                        Ok::<(), TypeProviderError>(())
                    }
                    CarrierWireOp::Change { content, .. } => {
                        record.lock().unwrap().push(format!("change:{content}"));
                        Ok(())
                    }
                    CarrierWireOp::Close => {
                        record.lock().unwrap().push("close".to_string());
                        if closes.fetch_add(1, Ordering::Relaxed) == 0 {
                            Err(TypeProviderError::new("wedged didClose"))
                        } else {
                            Ok(())
                        }
                    }
                }
            }
        }
    };

    // Open + commit.
    state
        .drive(carrier, PendingKind::Inject(Arc::from("v1")), make_sink())
        .await
        .expect("open ok");

    // The FAILED close — leaves a reconcilable shell + fails closed.
    let closed = state.drive(carrier, PendingKind::Close, make_sink()).await;
    assert!(closed.is_err(), "the failed close fails closed");

    // The RETRY inject — must reconcile the uncertain open, never blindly re-open it.
    state
        .drive(carrier, PendingKind::Inject(Arc::from("v2")), make_sink())
        .await
        .expect("the retry reconciles and re-opens");

    let wire = record.lock().unwrap().clone();
    // The reconcile pattern: the failed close, THEN a reconcile close, THEN a fresh open.
    assert_eq!(
        wire,
        vec![
            "open:v1".to_string(),
            "close".to_string(), // the failed top-level close
            "close".to_string(), // the reconcile didClose before the fresh open
            "open:v2".to_string(),
        ],
        "a failed close then a retry reconciles via didClose THEN a fresh didOpen"
    );
    // The bug this discriminates: NEVER a bare duplicate didOpen right after the failed close.
    assert_ne!(
        wire,
        vec![
            "open:v1".to_string(),
            "close".to_string(),
            "open:v2".to_string(),
        ],
        "the retry must NOT send a bare duplicate didOpen after a failed close (it reconciles first)"
    );
    // No two consecutive didOpens without an intervening reconcile didClose.
    assert_eq!(
        wire.windows(2)
            .filter(|w| w[0].starts_with("open") && w[1].starts_with("open"))
            .count(),
        0,
        "every didOpen after the first is preceded by a reconcile didClose (no duplicate open)"
    );
    // The retry's fresh open committed → the carrier is now synced and serveable.
    assert_eq!(
        state.synced_content(carrier, carrier).as_deref(),
        Some("v2"),
        "after the reconcile + fresh open the carrier is synced"
    );
}

/// A CANCELLED close (an outer overlay deadline drops the drive future mid-barrier) must NOT
/// drop the local slot to VACANT — the `didClose` may not have confirmed, so the shared
/// Program may still hold the doc open. The close transitions the slot to a non-serveable
/// `PossiblyOpenUnsynced` shell UP FRONT (before the await), so a cancel mid-close leaves that
/// reconcilable shell (never Vacant, never served); the cancelled close never completes, so it
/// does NOT prune the per-carrier state. A subsequent retry RECONCILES.
///
/// Discriminator: dropping the slot to VACANT UP FRONT (before the barrier await) would leave a
/// cancelled close's carrier vacant — a later inject then classifies it Vacant → Open and sends
/// a bare duplicate `didOpen`.
#[tokio::test]
async fn cancelled_close_leaves_reconcilable_shell_not_vacant() {
    use std::sync::Mutex as StdMutex;

    let state = CarrierSyncState::new();
    let carrier = "/ws/Hang.vue.tsx";

    // Open + commit the carrier (slot present + synced).
    state
        .drive(
            carrier,
            PendingKind::Inject(Arc::from("v1")),
            |_op: CarrierWireOp| async { Ok::<(), TypeProviderError>(()) },
        )
        .await
        .expect("open ok");
    assert_eq!(
        state.synced_content(carrier, carrier).as_deref(),
        Some("v1"),
        "the carrier is synced after the open"
    );

    // A close whose wire barrier NEVER answers, wrapped in a short OUTER timeout (an overlay
    // deadline). The timeout CANCELS the drive future mid-await — the up-front
    // `PossiblyOpenUnsynced` transition (before the await) is the safe state left behind.
    let hanging_close = state.drive(
        carrier,
        PendingKind::Close,
        |op: CarrierWireOp| async move {
            if let CarrierWireOp::Close = op {
                std::future::pending::<()>().await; // never answers
            }
            Ok::<(), TypeProviderError>(())
        },
    );
    let outcome = tokio::time::timeout(Duration::from_millis(100), hanging_close).await;
    assert!(
        outcome.is_err(),
        "the hanging close must be cancelled by the timeout"
    );

    // The slot is a reconcilable `PossiblyOpenUnsynced` shell — NOT vacant (never a later bare
    // duplicate didOpen), NOT served.
    assert!(
        matches!(
            state.injected.lock().get(carrier),
            Some(CarrierSlot::PossiblyOpenUnsynced)
        ),
        "a cancelled close leaves a PossiblyOpenUnsynced shell — never VACANT"
    );
    assert!(
        state.synced_content(carrier, carrier).is_none(),
        "the shell serves no content after a cancelled close (fail-closed)"
    );
    // The cancelled close never completed (its prune is after the await), so the per-carrier
    // gate + pending survive for the reconciling retry.
    assert!(
        state.gates.lock().contains_key(carrier),
        "a cancelled close does NOT prune the gate (it never completed)"
    );
    assert!(
        state.pending.lock().contains_key(carrier),
        "a cancelled close does NOT prune the pending cell"
    );

    // A RETRY inject reconciles the uncertain state: a bounded didClose THEN a fresh didOpen.
    let record: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
    {
        let record = Arc::clone(&record);
        state
            .drive(
                carrier,
                PendingKind::Inject(Arc::from("v2")),
                move |op: CarrierWireOp| {
                    let record = Arc::clone(&record);
                    async move {
                        match op {
                            CarrierWireOp::Open { content, .. } => {
                                record.lock().unwrap().push(format!("open:{content}"));
                                Ok::<(), TypeProviderError>(())
                            }
                            CarrierWireOp::Change { content, .. } => {
                                record.lock().unwrap().push(format!("change:{content}"));
                                Ok(())
                            }
                            CarrierWireOp::Close => {
                                record.lock().unwrap().push("close".to_string());
                                Ok(())
                            }
                        }
                    }
                },
            )
            .await
            .expect("the retry reconciles and re-opens");
    }
    let wire = record.lock().unwrap().clone();
    assert_eq!(
        wire,
        vec!["close".to_string(), "open:v2".to_string()],
        "the retry after a cancelled close reconciles (didClose THEN fresh didOpen) — never a \
         bare duplicate didOpen"
    );
    assert_eq!(
        state.synced_content(carrier, carrier).as_deref(),
        Some("v2"),
        "after the reconcile + fresh open the carrier is synced"
    );
}

/// A FIRST-OPEN inject that is CANCELLED mid-barrier (an outer overlay deadline drops the
/// drive future while it is parked on the sync barrier) must leave a fail-closed
/// `PossiblyOpenUnsynced` shell — the `didOpen` may have reached the Program, so the slot is
/// PRESENT but never serveable, and the next inject RECONCILES it (never a blind duplicate
/// `didOpen`). Pending accounting stays intact so the later op re-drives.
///
/// This characterizes the slot-shell invariant left by a cancel; the runnable regression
/// discriminator for the duplicate-`didOpen`-on-retry bug is
/// [`super::shared_tests::cancelled_first_open_retry_reconciles_via_close_then_fresh_open`].
#[tokio::test]
async fn cancelled_first_open_inject_leaves_possibly_open_unsynced_shell() {
    let state = CarrierSyncState::new();
    let carrier = "/ws/CancelOpen.vue.tsx";

    // An inject whose first-open barrier NEVER answers (parks forever at the sink await).
    let hanging = state.drive(
        carrier,
        PendingKind::Inject(Arc::from("v1")),
        |op: CarrierWireOp| async move {
            if let CarrierWireOp::Open { .. } = op {
                std::future::pending::<()>().await; // never answers → parks on the barrier
            }
            Ok::<(), TypeProviderError>(())
        },
    );
    // Poll far enough to reserve the slot + park on the barrier, then CANCEL (drop) the
    // future by letting the timeout elapse.
    let outcome = tokio::time::timeout(Duration::from_millis(100), hanging).await;
    assert!(
        outcome.is_err(),
        "the hanging first-open must be cancelled by the timeout"
    );

    // A PossiblyOpenUnsynced shell (PRESENT, never removed) — the uncertain wire state forces a
    // reconcile on the next inject rather than a blind duplicate didOpen.
    assert!(
        matches!(
            state.injected.lock().get(carrier),
            Some(CarrierSlot::PossiblyOpenUnsynced)
        ),
        "a cancelled first-open leaves a PossiblyOpenUnsynced shell (present, never removed)"
    );
    assert!(
        state.synced_content(carrier, carrier).is_none(),
        "the carrier serves no content after a cancelled first-open (fail-closed)"
    );
    // Pending accounting is UNCHANGED — the op was never committed, so a later gate holder
    // still finds it drainable and re-drives it correctly.
    let (_, kind) = state
        .take_drainable(carrier)
        .expect("the cancelled op's pending accounting is intact and still drainable");
    assert_eq!(
        drained_inject(&kind),
        "v1",
        "the cancelled op still carries its coalesced content for a later re-drive"
    );
}

/// A REFRESH (`didChange`) inject that is CANCELLED mid-barrier fails CLOSED to the
/// non-serveable `OpenUnsyncedContent` state: it must NOT keep serving the prior synced content
/// (a cancelled refresh is exactly the uncertain window — the `didChange` may already have
/// applied before its confirmation was lost) and must NOT serve the un-synced new content. The
/// rollback guard reconciles the cancelled refresh to `OpenUnsyncedContent`, and a later reserve
/// classifies the still-open doc as a fresh `Change` (never a close+reopen reconcile).
///
/// Discriminating invariant: a cancelled refresh must not restore the prior `Synced { v1 }` — both
/// the cancelled-refresh `synced_content == None` and the `OpenUnsyncedContent` slot assert the
/// doc's text is uncertain, so the prior synced content is never served again.
#[tokio::test]
async fn cancelled_refresh_inject_fails_closed_to_open_unsynced_content() {
    let state = CarrierSyncState::new();
    let carrier = "/ws/CancelRefresh.vue.tsx";

    // Pre-seed a SYNCED slot (prior content v1) via a completed first open.
    state
        .drive(
            carrier,
            PendingKind::Inject(Arc::from("v1")),
            |_op: CarrierWireOp| async { Ok::<(), TypeProviderError>(()) },
        )
        .await
        .expect("first open ok");
    assert_eq!(
        state.synced_content(carrier, carrier).as_deref(),
        Some("v1"),
        "the prior synced content is present before the refresh"
    );

    // A refresh (didChange) whose barrier NEVER answers, then CANCEL mid-barrier.
    let hanging = state.drive(
        carrier,
        PendingKind::Inject(Arc::from("v2")),
        |op: CarrierWireOp| async move {
            if let CarrierWireOp::Change { .. } = op {
                std::future::pending::<()>().await; // parks on the refresh barrier
            }
            Ok::<(), TypeProviderError>(())
        },
    );
    let outcome = tokio::time::timeout(Duration::from_millis(100), hanging).await;
    assert!(
        outcome.is_err(),
        "the hanging refresh must be cancelled by the timeout"
    );

    // The slot fails CLOSED to OpenUnsyncedContent — the prior v1 is NOT restored (it may now
    // mismatch the Program's actual text), and the un-synced v2 is never served either.
    assert!(
        matches!(
            state.injected.lock().get(carrier),
            Some(CarrierSlot::OpenUnsyncedContent)
        ),
        "a cancelled refresh fails closed to the non-serveable OpenUnsyncedContent slot"
    );
    assert_eq!(
        state.synced_content(carrier, carrier),
        None,
        "a cancelled refresh serves NOTHING — never the possibly-stale prior v1 (a cancelled \
         refresh must not serve the prior synced content), never the un-synced v2"
    );
    assert!(
        state.injected.lock().contains_key(carrier),
        "a refresh cancel keeps the carrier slot PRESENT (it fails closed, never removes)"
    );

    // The still-open doc reserves as a fresh Change on the retry — never a close+reopen reconcile.
    assert_eq!(
        reserve_carrier_capturing(&state.injected, carrier),
        InjectAction::Change,
        "an OpenUnsyncedContent slot retries via a FRESH didChange (the doc is open), never a \
         close+reopen reconcile"
    );
}

/// Waiter-aware pruning: a committed close must NOT prune its per-carrier gate while a
/// blocked waiter/holder still references the gate `Arc` — pruning it would let a later
/// reopen mint a SECOND `Arc`, transiently splitting the carrier across two live gates.
/// The prune is gated on `Arc::strong_count(gate) == 2` (the map entry + the draining op's
/// single local clone); any waiter raises the count and SKIPS the prune, so the map entry
/// survives and a reopen reuses the SAME `Arc`. With no waiter, a committed close prunes.
///
/// Discriminator: a close-arm prune observing only the sequence check would remove the gate
/// `Arc` even while a blocked waiter held a reference — a later reopen then mints a fresh `Arc`,
/// splitting one carrier across two.
#[test]
fn prune_is_waiter_aware_and_a_reopen_reuses_the_same_gate() {
    let state = CarrierSyncState::new();
    let carrier = "/ws/Split.vue.tsx";

    // The draining op's local gate clone: map(1) + draining(1) = strong_count 2 (no waiter).
    let draining_gate = state.gate_for(carrier);
    state.record_pending(carrier, 1, PendingKind::Close);
    state.mark_committed(carrier, 1);
    assert_eq!(
        Arc::strong_count(&draining_gate),
        2,
        "the map entry + the draining op's sole clone"
    );

    // ── (a) WITH a blocked waiter present: the prune must SKIP (no two-gate split). ──
    // A blocked waiter records its own `gate_for` clone before parking on `gate.lock()`.
    let waiter_gate = state.gate_for(carrier);
    assert_eq!(
        Arc::strong_count(&draining_gate),
        3,
        "map + the draining op + a blocked waiter"
    );
    state.prune_carrier_state_if_idle(carrier);
    assert!(
        state.gates.lock().contains_key(carrier),
        "a blocked waiter (strong_count 3) must SKIP the prune — the gate Arc stays live so \
         a reopen reuses it, never a second gate for one carrier"
    );
    // The reopen reuses the SAME `Arc` — the discriminating no-split proof.
    let reopen_gate = state.gate_for(carrier);
    assert!(
        Arc::ptr_eq(&draining_gate, &reopen_gate),
        "a reopen after a skipped prune reuses the SAME gate Arc (no split across two gates)"
    );

    // ── (b) NO waiter: a committed close prunes the gate + pending state. ──
    // Drop every extra clone so only the map entry + one draining clone remain.
    drop(waiter_gate);
    drop(reopen_gate);
    assert_eq!(
        Arc::strong_count(&draining_gate),
        2,
        "map + the sole draining clone remain once the waiter released"
    );
    state.record_pending(carrier, 2, PendingKind::Close);
    state.mark_committed(carrier, 2);
    state.prune_carrier_state_if_idle(carrier);
    assert!(
        !state.gates.lock().contains_key(carrier),
        "with NO waiter (strong_count 2), a committed close prunes the per-carrier gate"
    );
    assert!(
        !state.pending.lock().contains_key(carrier),
        "with NO waiter the pending cell is pruned too (the maps track the current open set)"
    );
}

/// Bounded cleanup shared by the close arm and the coalesced-away early-return path:
/// `prune_carrier_state_if_idle` prunes a carrier's per-carrier gate + pending state ONLY when
/// it is fully committed (`latest_seq <= committed_seq`), CLOSED (no injected slot), AND has no
/// waiter (`strong_count(gate) == 2`). Each of the three negatives — a newer uncommitted op, an
/// open injected slot, a blocked waiter — SKIPS the prune; only all-three-positive prunes.
#[test]
fn prune_carrier_state_prunes_only_when_committed_closed_no_waiter() {
    let state = CarrierSyncState::new();
    let carrier = "/ws/IdlePrune.vue.tsx";

    // The draining op's local gate clone: map(1) + draining(1) = strong_count 2 (no waiter).
    let draining_gate = state.gate_for(carrier);

    // ── (a) A newer UNCOMMITTED op ⇒ SKIP (there is still work to drain). ──
    state.record_pending(carrier, 1, PendingKind::Close);
    state.prune_carrier_state_if_idle(carrier);
    assert!(
        state.gates.lock().contains_key(carrier),
        "a not-fully-committed carrier (latest_seq > committed_seq) must SKIP the prune"
    );

    // Commit it → fully committed. The carrier was never opened, so it has no injected slot.
    state.mark_committed(carrier, 1);
    assert_eq!(
        Arc::strong_count(&draining_gate),
        2,
        "the map entry + the draining op's sole clone"
    );

    // ── (b) An OPEN injected slot ⇒ SKIP (the carrier is not closed). ──
    state.injected.lock().insert(
        carrier.to_string(),
        CarrierSlot::Synced {
            content: Arc::from("open"),
        },
    );
    state.prune_carrier_state_if_idle(carrier);
    assert!(
        state.gates.lock().contains_key(carrier),
        "an OPEN carrier (injected slot present) must SKIP the prune"
    );
    state.injected.lock().remove(carrier);

    // ── (c) A blocked WAITER (strong_count 3) ⇒ SKIP (no two-gate split). ──
    let waiter_gate = state.gate_for(carrier);
    assert_eq!(
        Arc::strong_count(&draining_gate),
        3,
        "map + the draining op + a blocked waiter"
    );
    state.prune_carrier_state_if_idle(carrier);
    assert!(
        state.gates.lock().contains_key(carrier),
        "a blocked waiter (strong_count 3) must SKIP the prune — the gate Arc stays live so a \
         reopen reuses it, never a second gate for one carrier"
    );
    drop(waiter_gate);

    // ── (d) Fully committed + closed + no waiter (strong_count 2) ⇒ PRUNE. ──
    assert_eq!(
        Arc::strong_count(&draining_gate),
        2,
        "the map entry + the sole draining clone remain once the waiter released"
    );
    state.prune_carrier_state_if_idle(carrier);
    assert!(
        !state.gates.lock().contains_key(carrier),
        "fully committed + closed + no waiter ⇒ the coalesced-away no-drain prunes the gate"
    );
    assert!(
        !state.pending.lock().contains_key(carrier),
        "the pending cell is pruned too (bounded cleanup, no retained closed state)"
    );
}

/// The KEY first-open cancellation regression: a cancelled first-open leaves the carrier's
/// wire state UNCERTAIN (the `didOpen` may have reached the shared Program), so a RETRY must
/// RECONCILE the shared Program to a known-closed state (a bounded `didClose`) BEFORE a fresh
/// `didOpen` — NEVER a duplicate bare `didOpen`, NEVER a `didChange` onto an unconfirmed open.
///
/// Discriminating invariant: a cancelled first-open must NOT strand the carrier vacant — a retry
/// that saw a vacant carrier would classify it Vacant → Open and send a SECOND bare `didOpen`
/// (wire `[open, open]`, no intervening `didClose`), a duplicate open on the shared Program. The
/// cancel leaves `PossiblyOpenUnsynced`, so the retry reconciles (`[open, close, open]`).
#[tokio::test]
async fn cancelled_first_open_retry_reconciles_via_close_then_fresh_open() {
    use std::sync::Mutex as StdMutex;

    let state = CarrierSyncState::new();
    let carrier = "/ws/RetryReconcile.vue.tsx";
    let record: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));

    // Drive 1: a first-open whose Open barrier NEVER answers, cancelled mid-barrier by an
    // outer timeout (an overlay deadline dropping the drive future while parked on the
    // barrier). The `didOpen` was already SENT — the shared Program may hold it.
    {
        let record = Arc::clone(&record);
        let hanging = state.drive(
            carrier,
            PendingKind::Inject(Arc::from("v1")),
            move |op: CarrierWireOp| {
                let record = Arc::clone(&record);
                async move {
                    match op {
                        CarrierWireOp::Open { content, .. } => {
                            record.lock().unwrap().push(format!("open:{content}"));
                            std::future::pending::<()>().await; // park on the barrier
                            Ok::<(), TypeProviderError>(())
                        }
                        CarrierWireOp::Change { content, .. } => {
                            record.lock().unwrap().push(format!("change:{content}"));
                            Ok(())
                        }
                        CarrierWireOp::Close => {
                            record.lock().unwrap().push("close".to_string());
                            Ok(())
                        }
                    }
                }
            },
        );
        let outcome = tokio::time::timeout(Duration::from_millis(100), hanging).await;
        assert!(
            outcome.is_err(),
            "the hanging first-open must be cancelled by the timeout"
        );
    }

    // After the cancel: the carrier serves NO content (fail-closed) — the `didOpen`'s wire
    // state is uncertain, so it is never served.
    assert!(
        state.synced_content(carrier, carrier).is_none(),
        "a cancelled first-open serves no content (fail-closed)"
    );

    // Drive 2: a RETRY. Because the prior first-open's wire state is UNCERTAIN, the retry must
    // RECONCILE — a bounded `didClose` FIRST, THEN a fresh `didOpen`.
    {
        let record = Arc::clone(&record);
        state
            .drive(
                carrier,
                PendingKind::Inject(Arc::from("v1")),
                move |op: CarrierWireOp| {
                    let record = Arc::clone(&record);
                    async move {
                        match op {
                            CarrierWireOp::Open { content, .. } => {
                                record.lock().unwrap().push(format!("open:{content}"));
                                Ok::<(), TypeProviderError>(())
                            }
                            CarrierWireOp::Change { content, .. } => {
                                record.lock().unwrap().push(format!("change:{content}"));
                                Ok(())
                            }
                            CarrierWireOp::Close => {
                                record.lock().unwrap().push("close".to_string());
                                Ok(())
                            }
                        }
                    }
                },
            )
            .await
            .expect("the retry reconciles and re-opens");
    }

    let wire = record.lock().unwrap().clone();
    // The full wire sequence: the cancelled open, a reconcile close, then a FRESH open.
    assert_eq!(
        wire,
        vec![
            "open:v1".to_string(),
            "close".to_string(),
            "open:v1".to_string()
        ],
        "the retry reconciles the uncertain open via didClose THEN a fresh didOpen — never a \
         duplicate bare didOpen, never a didChange"
    );
    // No `didChange` onto an unconfirmed open.
    assert!(
        !wire.iter().any(|op| op.starts_with("change")),
        "the retry never sends a didChange onto an unconfirmed open"
    );
    // No two consecutive `didOpen`s without an intervening `didClose` (no duplicate bare open).
    assert_eq!(
        wire.windows(2)
            .filter(|w| w[0].starts_with("open") && w[1].starts_with("open"))
            .count(),
        0,
        "every didOpen after the first is preceded by a reconcile didClose (no duplicate open)"
    );
    // The retry's fresh open committed → the carrier is now synced and serveable.
    assert_eq!(
        state.synced_content(carrier, carrier).as_deref(),
        Some("v1"),
        "after the reconcile + fresh open the carrier is synced and serveable"
    );
}

/// A real async `drive`-level race for bounded cleanup: a first close BLOCKS in its sink
/// (holding the gate); two more closes queue behind it on the SAME gate; when the blocked
/// close releases, the first waiter drains the coalesced-latest close (SKIPPING its own prune
/// while the second waiter still references the gate `Arc`), and the LAST waiter finds nothing
/// to drain (coalesced away) and prunes the now-idle, fully-committed, closed, no-waiter
/// carrier on the early-return path. Proves: exactly ONE live gate while ops queue (no
/// two-gate split), and NO closed-carrier `gates`/`pending` entry is retained forever after a
/// coalesced-away op.
///
/// Discriminator: a last-waiter no-drain that returned `Ok(())` WITHOUT pruning (while the
/// drainer had SKIPPED its own prune because that waiter held the gate `Arc`) would leave the
/// `gates`/`pending` entries for a fully-closed carrier alive indefinitely.
#[tokio::test]
async fn coalesced_away_no_drain_prunes_idle_gate_and_pending_state() {
    use std::sync::Mutex as StdMutex;

    let state = Arc::new(CarrierSyncState::new());
    let carrier = "/ws/NoDrainCleanup.vue.tsx";
    let record: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
    let close_entered = Arc::new(tokio::sync::Notify::new());
    let release_close = Arc::new(tokio::sync::Notify::new());

    // Open + commit first, so the FIRST close has an open slot to retract and blocks in the
    // sink. A close only touches the sink when it actually retracts an open slot; the later
    // coalesced closes find the slot already gone and never touch the sink (only the first
    // close ever blocks).
    state
        .drive(
            carrier,
            PendingKind::Inject(Arc::from("v1")),
            |_op: CarrierWireOp| async { Ok::<(), TypeProviderError>(()) },
        )
        .await
        .expect("open ok");

    let make_sink = || {
        let record = Arc::clone(&record);
        let close_entered = Arc::clone(&close_entered);
        let release_close = Arc::clone(&release_close);
        move |op: CarrierWireOp| {
            let record = Arc::clone(&record);
            let close_entered = Arc::clone(&close_entered);
            let release_close = Arc::clone(&release_close);
            async move {
                match op {
                    CarrierWireOp::Close => {
                        record.lock().unwrap().push("close".to_string());
                        close_entered.notify_one();
                        release_close.notified().await;
                        Ok::<(), TypeProviderError>(())
                    }
                    CarrierWireOp::Open { content, .. } => {
                        record.lock().unwrap().push(format!("open:{content}"));
                        Ok(())
                    }
                    CarrierWireOp::Change { content, .. } => {
                        record.lock().unwrap().push(format!("change:{content}"));
                        Ok(())
                    }
                }
            }
        }
    };

    // A: the first close — drains its close op, retracts the open slot, enters the sink and
    // HOLDS the gate.
    let a = {
        let state = Arc::clone(&state);
        let sink = make_sink();
        tokio::spawn(async move { state.drive(carrier, PendingKind::Close, sink).await })
    };
    close_entered.notified().await;

    // D then E: two more closes queued behind A on the SAME gate. Each records its pending op
    // (raising the newest seq) and blocks on the gate; their eventual drains find the slot
    // already retracted (no sink call), so only A ever blocks. D is queued before E (FIFO).
    let d = {
        let state = Arc::clone(&state);
        let sink = make_sink();
        tokio::spawn(async move { state.drive(carrier, PendingKind::Close, sink).await })
    };
    tokio::time::sleep(Duration::from_millis(30)).await;
    let e = {
        let state = Arc::clone(&state);
        let sink = make_sink();
        tokio::spawn(async move { state.drive(carrier, PendingKind::Close, sink).await })
    };
    tokio::time::sleep(Duration::from_millis(60)).await;

    // While A holds the gate and D, E wait, the carrier is served by exactly ONE gate — never
    // split across two live gates.
    assert_eq!(
        state.gates.lock().len(),
        1,
        "one carrier is served by exactly ONE gate while ops queue behind it (no two-gate split)"
    );

    // Release A: A commits and SKIPS its prune (a newer op is queued / a waiter holds the gate
    // Arc); D drains the coalesced-latest close (a no-op retract — the slot is already gone)
    // and SKIPS its prune (E still waits); E finds nothing to drain (coalesced away) and, on
    // the early-return path, prunes the now-idle, fully-committed, closed, no-waiter carrier.
    release_close.notify_one();
    a.await.unwrap().expect("close A ok");
    d.await.unwrap().expect("close D ok");
    e.await.unwrap().expect("close E ok");

    // Bounded cleanup: the coalesced-away no-drain pruned the per-carrier gate + pending state
    // — no closed-carrier entry is retained forever.
    assert!(
        !state.gates.lock().contains_key(carrier),
        "the coalesced-away no-drain prunes the per-carrier gate (bounded cleanup, no retained \
         closed state)"
    );
    assert!(
        !state.pending.lock().contains_key(carrier),
        "the coalesced-away no-drain prunes the per-carrier pending cell (bounded cleanup)"
    );
    assert!(
        !state.injected.lock().contains_key(carrier),
        "the carrier is closed — no injected slot survives"
    );
    // Exactly ONE didClose reached the wire (A's retract); D and E coalesced away with no extra
    // wire op (never a spurious reopen or duplicate close).
    assert_eq!(
        *record.lock().unwrap(),
        vec!["close".to_string()],
        "only the first close retracts the open slot; the coalesced closes emit no extra wire op"
    );
}

/// The bounded-close INTERNAL timeout FIRES: a `didClose` whose sink NEVER resolves, driven with
/// a SHORT injected `close_barrier_bound` and NO outer deadline on the `drive(Close)`, must return
/// a fail-closed `Err` WITHIN the bound (never hang the per-carrier gate), leave the slot the
/// reconcilable `PossiblyOpenUnsynced` shell, and RELEASE the gate so a subsequent op proceeds.
///
/// This is non-vacuous: the `drive(Close)` below has NO outer bound, so without the internal
/// `tokio::time::timeout` wrapper in `bounded_carrier_close_with_timeout` an unwrapped
/// `sink(Close)` on a `pending()` future would hang forever — never returning `Err`, never
/// releasing the gate. The internal wrapper honouring the injected short bound is what returns
/// `Err` in-test, so the assertions below observe the internal timeout firing rather than an
/// outer deadline.
#[tokio::test]
async fn bounded_close_internal_timeout_fires_and_releases_gate() {
    use std::time::Instant;

    // A SHORT injected close bound — the internal wrapper must honour it, not the 10s default.
    let state = CarrierSyncState::with_close_barrier_bound(Duration::from_millis(50));
    let carrier = "/ws/WedgedClose.vue.tsx";

    // Open + commit so the close has an open slot to retract (the close reaches the sink).
    state
        .drive(
            carrier,
            PendingKind::Inject(Arc::from("v1")),
            |_op: CarrierWireOp| async { Ok::<(), TypeProviderError>(()) },
        )
        .await
        .expect("open ok");

    // A close whose `didClose` NEVER resolves — driven with NO outer deadline. The INTERNAL bound
    // is the ONLY thing that can return it.
    let start = Instant::now();
    let closed = state
        .drive(
            carrier,
            PendingKind::Close,
            |op: CarrierWireOp| async move {
                if let CarrierWireOp::Close = op {
                    std::future::pending::<()>().await; // never answers
                }
                Ok::<(), TypeProviderError>(())
            },
        )
        .await;
    let elapsed = start.elapsed();

    assert!(
        closed.is_err(),
        "a never-answering close must fail closed via the INTERNAL bound (never hang)"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "the internal ~50ms bound fired, not the 10s default or an unbounded await; elapsed \
         {elapsed:?}"
    );
    // The slot stays a reconcilable PossiblyOpenUnsynced shell — never Vacant, never served.
    assert!(
        matches!(
            state.injected.lock().get(carrier),
            Some(CarrierSlot::PossiblyOpenUnsynced)
        ),
        "a bounded-out close leaves the reconcilable PossiblyOpenUnsynced shell"
    );
    assert!(
        state.synced_content(carrier, carrier).is_none(),
        "the shell serves no content after a bounded-out close (fail-closed)"
    );

    // The gate was RELEASED: a subsequent op proceeds (reconcile + reopen) and does NOT deadlock.
    // Bound it defensively so a gate-still-held regression fails fast instead of hanging the suite.
    let subsequent = tokio::time::timeout(
        Duration::from_secs(2),
        state.drive(
            carrier,
            PendingKind::Inject(Arc::from("v2")),
            |_op: CarrierWireOp| async { Ok::<(), TypeProviderError>(()) },
        ),
    )
    .await;
    assert!(
        matches!(subsequent, Ok(Ok(()))),
        "the bounded-out close RELEASED the gate — a subsequent op proceeds, never deadlocks"
    );
    assert_eq!(
        state.synced_content(carrier, carrier).as_deref(),
        Some("v2"),
        "the subsequent op reconciled the shell and re-opened the latest content"
    );
}

/// The reconcile-close-FAILURE abort arm: an inject over a `PossiblyOpenUnsynced` shell first
/// sends a BOUNDED reconcile `didClose`; when that reconcile close FAILS, the open is ABORTED
/// (never proceed to the fresh `didOpen`) and the drive fails closed to OWNED, leaving the shell
/// reconcilable for the next retry. The wire records `[close]` ONLY — the fresh `didOpen` is
/// never sent onto a possibly-still-open Program.
///
/// This is non-vacuous: were the abort arm to fall through to the open (dropping the
/// `return Err`), the wire would record `[close, open:v1]` and the drive would return `Ok` — the
/// bare-duplicate-`didOpen` defect (a `didOpen` onto an un-reconciled, possibly-open Program
/// file). Every OTHER `ReconcileThenOpen` drive-test drives the reconcile close to SUCCESS, so
/// the `[close]`-only wire and the fail-closed `Err` asserted below are what lock this failure
/// arm.
#[tokio::test]
async fn reconcile_close_failure_aborts_the_fresh_open() {
    use std::sync::Mutex as StdMutex;

    let state = CarrierSyncState::new();
    let carrier = "/ws/ReconcileAbort.vue.tsx";

    // Seed a PossiblyOpenUnsynced shell (a prior first-open of uncertain wire state).
    state
        .injected
        .lock()
        .insert(carrier.to_string(), CarrierSlot::PossiblyOpenUnsynced);

    let record: Arc<StdMutex<Vec<String>>> = Arc::new(StdMutex::new(Vec::new()));
    let sink = {
        let record = Arc::clone(&record);
        move |op: CarrierWireOp| {
            let record = Arc::clone(&record);
            async move {
                match op {
                    // The reconcile didClose FAILS (the shared Program may still hold the doc open).
                    CarrierWireOp::Close => {
                        record.lock().unwrap().push("close".to_string());
                        Err(TypeProviderError::new("wedged reconcile didClose"))
                    }
                    CarrierWireOp::Open { content, .. } => {
                        record.lock().unwrap().push(format!("open:{content}"));
                        Ok::<(), TypeProviderError>(())
                    }
                    CarrierWireOp::Change { content, .. } => {
                        record.lock().unwrap().push(format!("change:{content}"));
                        Ok(())
                    }
                }
            }
        }
    };

    let result = state
        .drive(carrier, PendingKind::Inject(Arc::from("v1")), sink)
        .await;

    // Fail closed to OWNED — never a silent Ok.
    assert!(
        result.is_err(),
        "a reconcile-close FAILURE aborts the open and fails closed (Err), never a silent Ok"
    );
    // ONLY the reconcile close reached the wire — the fresh didOpen was ABORTED.
    let wire = record.lock().unwrap().clone();
    assert_eq!(
        wire,
        vec!["close".to_string()],
        "ONLY the reconcile didClose is sent — the fresh didOpen is aborted after the reconcile \
         close fails (never `[close, open]`)"
    );
    assert!(
        !wire.iter().any(|op| op.starts_with("open")),
        "the fresh didOpen must NOT be sent onto a possibly-still-open Program after a failed \
         reconcile close (the bare-duplicate-didOpen defect)"
    );
    // The slot stays a reconcilable PossiblyOpenUnsynced shell — the next retry re-reconciles.
    assert!(
        matches!(
            state.injected.lock().get(carrier),
            Some(CarrierSlot::PossiblyOpenUnsynced)
        ),
        "a failed reconcile close leaves the reconcilable PossiblyOpenUnsynced shell (never \
         Vacant, never serving)"
    );
    assert!(
        state.synced_content(carrier, carrier).is_none(),
        "the shell serves no content after a failed reconcile close (fail-closed)"
    );
}
