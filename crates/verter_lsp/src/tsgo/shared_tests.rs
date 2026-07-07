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
    apply_local_sync_commit, decide_shared_serve, promote_synced, reserve_carrier,
    resolve_editor_binding, stable_project_identity, sync_commit, synced_content, CarrierSlot,
    CarrierSyncState, CarrierWireOp, InjectAction, PendingKind, SharedModeController, SyncCommit,
    SyncMutex,
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
/// observes the reserved slot (`InjectAction::Change`). A check-then-await inject would
/// let multiple threads both observe "absent" and both send `didOpen` version 1.
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
            if reserve_carrier(&injected, "/ws/Foo.vue.tsx") == InjectAction::Open {
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
         version 1; every other must send didChange"
    );
    assert!(
        injected.lock().contains_key("/ws/Foo.vue.tsx"),
        "the carrier remains reserved after the race"
    );
}

/// A second reservation of an already-present carrier is a `Change` (never a second
/// `Open`). Reservation itself NEVER sets the slot's SYNCED content — the reserved
/// text is not served until its barrier is confirmed accepted ([`promote_synced`]).
#[test]
fn reserve_of_present_carrier_is_change() {
    use std::collections::HashMap;
    let injected = parking_lot::Mutex::new(HashMap::<String, CarrierSlot>::new());
    assert_eq!(
        reserve_carrier(&injected, "/ws/A.vue.tsx"),
        InjectAction::Open,
        "the first reservation is Open"
    );
    assert_eq!(
        reserve_carrier(&injected, "/ws/A.vue.tsx"),
        InjectAction::Change,
        "a subsequent reservation of the same carrier is Change"
    );
    // Reservation reserves the slot but commits NO synced content — the reserved
    // text is served only after its sync barrier is confirmed accepted.
    assert_eq!(
        synced_content(&injected, "/ws/A.vue.tsx", "/ws/A.vue.tsx"),
        None,
        "a reserved-but-not-yet-synced carrier serves no content"
    );
}

/// The sync-outcome consistency oracle. A barrier SUCCESS promotes; a
/// FIRST-OPEN failure retracts the possibly-open Program file; a `didChange` failure
/// keeps the prior synced content. RED before the fix: the reserved text was
/// committed to the served index regardless of the barrier outcome (`Promote`
/// always), so a failed barrier left divergent served state.
#[test]
fn sync_commit_maps_barrier_outcome_to_consistent_action() {
    assert_eq!(sync_commit(InjectAction::Open, true), SyncCommit::Promote);
    assert_eq!(sync_commit(InjectAction::Change, true), SyncCommit::Promote);
    assert_eq!(
        sync_commit(InjectAction::Open, false),
        SyncCommit::RetractOpen,
        "a first-open barrier failure must RETRACT the possibly-open Program file (no phantom open)"
    );
    assert_eq!(
        sync_commit(InjectAction::Change, false),
        SyncCommit::KeepPriorSynced,
        "a didChange barrier failure must KEEP the prior synced content (never the unaccepted text)"
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
    assert_eq!(reserve_carrier(&injected, carrier), InjectAction::Open);
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

/// A `didChange` whose sync barrier FAILS/times out does NOT leave `injected`
/// serving the unaccepted new text: the served content stays the PRIOR synced value
/// the shared Program still holds.
#[test]
fn didchange_failure_keeps_prior_synced_content() {
    use std::collections::HashMap;
    let injected = parking_lot::Mutex::new(HashMap::<String, CarrierSlot>::new());
    let carrier = "/ws/A.vue.tsx";

    // First open syncs v1.
    assert_eq!(reserve_carrier(&injected, carrier), InjectAction::Open);
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

    // A didChange for v2 whose barrier FAILS: the served content stays v1.
    assert_eq!(reserve_carrier(&injected, carrier), InjectAction::Change);
    apply_local_sync_commit(
        &injected,
        carrier,
        Arc::from("v2"),
        sync_commit(InjectAction::Change, false),
    );
    assert_eq!(
        synced_content(&injected, carrier, carrier).as_deref(),
        Some("v1"),
        "a failed didChange barrier keeps the PRIOR synced content, never the unaccepted v2"
    );
}

/// A FIRST-OPEN whose sync barrier FAILS/times out drops the local slot (the
/// caller separately retracts the Program open), so a later open re-reserves cleanly
/// as `Open` — never a phantom `Change` for a carrier the Program never accepted.
#[test]
fn open_failure_retracts_and_drops_slot() {
    use std::collections::HashMap;
    let injected = parking_lot::Mutex::new(HashMap::<String, CarrierSlot>::new());
    let carrier = "/ws/A.vue.tsx";

    assert_eq!(reserve_carrier(&injected, carrier), InjectAction::Open);
    // The first-open barrier FAILS → the RetractOpen commit drops the local slot.
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
        !injected.lock().contains_key(carrier),
        "a failed first-open must drop the local slot (no phantom reservation)"
    );
    // A later open re-reserves cleanly as Open (not a phantom Change).
    assert_eq!(
        reserve_carrier(&injected, carrier),
        InjectAction::Open,
        "after a dropped first-open the carrier re-reserves as Open, not a phantom Change"
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
/// and the latest content is served after. RED without the gate: the Change wire-sends
/// while the Open barrier is still in flight (a didChange ahead of the didOpen).
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

/// The exact desync: a first-Open TIMEOUT must not drop a slot a later injection
/// committed. With serialization, the failed first-open retracts + drops its slot, then
/// the queued later injection re-OPENS the latest content (slot vacant) and commits —
/// the stale earlier op never clobbers the committed later state. RED without the gate:
/// the concurrent first-open timeout's retract/drop races the later op's promote,
/// leaving the overlay desynced (slot dropped despite a committed change).
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

    // The failed first-open retracted + dropped its slot; B then re-OPENED (slot
    // vacant) the LATEST content — the failed earlier op never clobbered B's commit.
    assert_eq!(
        *record.lock().unwrap(),
        vec![
            "open:v1".to_string(),
            "retract".to_string(),
            "open:v2".to_string()
        ],
        "ordered: the failed first-open retracts, then the later op re-opens the latest content"
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
/// genuine reopen). RED before unifying close into the gate: close bypassed the pending
/// cell, so a queued injection could still drain and reopen a just-closed carrier.
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
/// RED without routing close through the gate: the close sends `didClose` immediately
/// (bypassing the per-carrier gate), so the wire records a `didClose` while the
/// `didOpen` barrier is still in flight — the op-around-close ordering violation that
/// can leak/reopen a closed carrier.
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

/// E5: closing a carrier PRUNES its per-carrier gate + pending state (not just the
/// injected slot), so the per-carrier maps track the CURRENT open set, not the
/// cumulative touched set across a long opt-in session. RED before the prune:
/// `gate_for` / `record_pending` inserted entries that close never removed, so the
/// gates/pending maps grew monotonically across open/close churn.
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

/// E5 race-safety: a close does NOT prune the gate/pending when a NEWER op is already
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

/// E1: a bounded/cancelled close leaves NO stale slot. The off-path retract wraps the
/// close in a timeout; if the close's wire barrier never answers and the timeout
/// CANCELS the drive future mid-await, the local slot must STILL be dropped (the
/// carrier reads not-synced) — the local view can never outlive a cancelled wire close.
/// RED before the fix: the slot was dropped AFTER the awaited close sink, so a
/// cancelled close skipped the drop and left a stale synced slot.
#[tokio::test]
async fn timed_out_close_still_drops_the_local_slot() {
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

    // A close whose wire barrier NEVER answers, wrapped in a short timeout (the off-path
    // bounded retract). The timeout CANCELS the drive future mid-await.
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

    // The local slot is dropped REGARDLESS of the cancelled wire close — the carrier
    // reads not-synced (no stale slot survives a timed-out close).
    assert!(
        state.synced_content(carrier, carrier).is_none(),
        "a timed-out close must still drop the local slot (E1 timeout-safe close)"
    );
    assert!(
        !state.injected.lock().contains_key(carrier),
        "no stale injected slot survives the cancelled close"
    );
}
