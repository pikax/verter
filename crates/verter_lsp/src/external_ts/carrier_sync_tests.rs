//! Foundation tests for the sealed carrier-sync gateway.
//!
//! The full owned-publish / owner-loss-retract behaviour is covered through the real
//! production-path harness in the workspace-scanner and server tests; these unit
//! tests pin the gateway's local primitives: the receipt-gated commit and the
//! close-only target helper.

use super::*;
use crate::provider_sync::{
    CarrierCommitStamp, ProviderOwnerBinding, ProviderPathKind, ProviderSyncState,
};
use dashmap::DashMap;
use std::sync::Arc;

use verter_session::external_ts::{
    AmbiguityCause, CarrierOwnershipResolution, EnvDims, ExternalTsProjectResolver, ProjectBinding,
    WorkspaceProjectResolver,
};
use verter_session::file_artifact_store::ProjectIdentity;
use verter_session::{CompileProfile, FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::canonical_path::CanonicalPath;
use verter_workspace::config::{
    load_compiler_options, load_project_membership, load_project_references,
};
use verter_workspace::membership::ConfiguredMembership;
use verter_workspace::memory::{MemoryOptions, MemoryWorkspace};
use verter_workspace::published_state::PublishedRoot;
use verter_workspace::snapshot_builder::{
    build_workspace_snapshot_simple, membership_to_spec, supported_extensions_for,
};
use verter_workspace::workspace_snapshot::{
    OwnershipProject, ProjectId, ProjectPayload, SnapshotGeneration, WorkspaceSnapshot,
};

use crate::external_ts::tsserver_backend::TsserverEngineBackend;
use crate::external_ts::{
    default_carrier_store_host_version, CarrierCompanion, CarrierPublishCoordinator,
    CarrierPublishStore, ReconcileOutcome, ReconcileReason,
};
use crate::project_resolver::{IdeProjectConfig, NativeProjectResolver};
use crate::provider_surface_store::ProviderSurfaceStore;
use crate::type_provider::mock::MockTypeProvider;
use crate::workspace_scanner::{classify_from_snapshot, Tier};

fn owned_carrier_state() -> ProviderSyncState {
    ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_string()),
        ide_path: Some("/workspace/src/App.vue.tsx".to_string()),
        api_path: Some("/workspace/src/App.vue.verter.ts".to_string()),
        decl_path: None,
        shadow_path: None,
        ide_background_loaded: true,
        api_background_loaded: true,
        decl_background_loaded: false,
        shadow_background_loaded: false,
        committed_ide_surface: None,
        commit_stamp: None,
    }
}

/// Admit a carrier state through a FRESH coordinator (owner-loss barrier at 0). The
/// single-commit tests below do not exercise the owner-loss barrier or a superseding peer;
/// the F1 equal-key and F5 vacant-resurrection tests use an explicit SHARED coordinator so
/// the barrier / prior stamp persist across commits.
fn commit_carrier_provider_state(
    states: &DashMap<String, ProviderSyncState>,
    canonical_id: &str,
    state: ProviderSyncState,
    receipt: &ProviderReadyReceipt,
) {
    let _ = CarrierTransactionCoordinator::new().admit_owned(states, canonical_id, state, receipt);
}

/// A resolved `ProjectBinding` (test-only seam) — the structural token a
/// [`ProviderReadyReceipt`] mint requires. Inert env dims (the receipt only records
/// them); `tsconfig` identifies the owning project.
fn test_binding(tsconfig: &str) -> ProjectBinding {
    let env_dims = EnvDims {
        parse_env_hash: [0u8; 16],
        resolve_env_hash: [0u8; 16],
        lib_env_hash: [0u8; 16],
        project_identity: ProjectIdentity([0u8; 16]),
    };
    ProjectBinding::new_for_test(
        "/workspace",
        tsconfig,
        "5.9.0",
        env_dims,
        Vec::new(),
        ProjectId(0),
        SnapshotGeneration(1),
    )
}

#[test]
fn carrier_source_revision_tracks_the_host_content_authority() {
    let canonical = "/workspace/src/App.vue";
    let workspace: Arc<dyn verter_workspace::WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions {
            roots: vec!["/workspace".to_string()],
            default_resolve_extensions: None,
        }));
    let host = VerterHost::new(HostConfig::default(), workspace);

    assert_eq!(carrier_source_revision(&host, canonical), 0);
    host.notify_upsert(
        canonical,
        Arc::<str>::from("<script>let count = 1;</script>"),
    );
    let first = carrier_source_revision(&host, canonical);
    assert!(
        first > 0,
        "the first host content transition advances the receipt revision"
    );

    host.notify_upsert(
        canonical,
        Arc::<str>::from("<script>let count = 2;</script>"),
    );
    assert!(
        carrier_source_revision(&host, canonical) > first,
        "a subsequent edit must mint a strictly newer carrier receipt"
    );
}

#[test]
fn commit_carrier_provider_state_requires_a_receipt_and_commits() {
    // The receipt-gated commit writes the carrier state into the shared map. The
    // receipt is the capability token minted from a resolved binding; without it this
    // call would not compile (the structural half of the fusion — the gateway is the
    // only production producer).
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let binding = test_binding("/workspace/tsconfig.json");
    let receipt = ProviderReadyReceipt::for_test(&binding);
    let state = owned_carrier_state();

    commit_carrier_provider_state(&states, "/workspace/src/App.vue", state.clone(), &receipt);

    let committed = states
        .get("/workspace/src/App.vue")
        .expect("the receipt-gated commit must write the carrier state");
    assert_eq!(
        committed.owner_binding,
        ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_string()),
    );
    assert_eq!(
        committed.ide_path.as_deref(),
        Some("/workspace/src/App.vue.tsx")
    );
    assert_eq!(
        committed.api_path.as_deref(),
        Some("/workspace/src/App.vue.verter.ts")
    );
}

#[test]
fn commit_carrier_provider_state_refuses_a_cross_owner_receipt() {
    // The receipt is a REAL fence, not a decorative capability token: the commit
    // validates that the receipt attests the owned state being committed. A receipt
    // minted for one owning project must NOT commit a state owned by a DIFFERENT project
    // (a stale or cross-owner clone). Pre-fix the commit ignored the receipt entirely
    // (`_receipt`), so this state WOULD be committed — this test discriminates that
    // regression.
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    // The receipt attests owner A.
    let receipt = ProviderReadyReceipt::for_test(&test_binding("/workspace/a/tsconfig.json"));
    // The state is owned by a DIFFERENT project B.
    let mut state = owned_carrier_state();
    state.owner_binding = ProviderOwnerBinding::Owned("/workspace/b/tsconfig.json".to_string());

    commit_carrier_provider_state(&states, "/workspace/src/App.vue", state, &receipt);

    assert!(
        states.get("/workspace/src/App.vue").is_none(),
        "a receipt attesting owner A must NOT commit a state owned by project B — the \
         cross-owner/stale receipt is refused by the owner-binding validation"
    );
}

#[test]
fn commit_carrier_provider_state_admits_a_matching_owner_receipt() {
    // The negative companion to the cross-owner refusal: a receipt whose owning tsconfig
    // EQUALS the state's owner key commits normally (the invariant the gateway always
    // satisfies, since it builds both from the same resolved binding).
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let receipt = ProviderReadyReceipt::for_test(&test_binding("/workspace/tsconfig.json"));
    let state = owned_carrier_state();

    commit_carrier_provider_state(&states, "/workspace/src/App.vue", state, &receipt);

    assert!(
        states.get("/workspace/src/App.vue").is_some(),
        "a receipt attesting the SAME owner as the state must commit"
    );
}

/// A `CarrierCompanion` for the IDE role at `uri` carrying `content` + optional map.
fn ide_companion(
    uri: &str,
    content: &str,
    map_json: Option<&str>,
    version: u64,
) -> CarrierCompanion {
    CarrierCompanion {
        provider_uri: Arc::from(uri),
        content: Arc::from(content),
        map_json: map_json.map(Arc::from),
        role: verter_session::external_ts::SnapshotRole::CarrierIde,
        script_kind: verter_session::external_ts::ScriptKind::Tsx,
        version,
    }
}

#[test]
fn commit_stamps_committed_ide_surface_and_gates_uncommitted_capture() {
    // The receipt-gated commit installs the receipt-attested committed IDE-surface
    // identity onto the state, and a capture is authorized ONLY for that exact
    // published surface. A newly-recorded-but-UNCOMMITTED surface (the record-before-
    // publish window that PERSISTS after a failed reconcile) carries a different
    // content/map identity and must NOT be capturable — mapping provider offsets
    // (produced against the last published content) through it would be wrong.
    //
    // DISCRIMINATING: removing the content/map match from
    // `authorizes_carrier_ide_capture` (making an OWNED state authorize any surface)
    // flips the two negative assertions from `false` to `true`.
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let binding = test_binding("/workspace/tsconfig.json");
    // The receipt attests the CarrierIde companion at the state's committed ide_path.
    let ide_uri = "/workspace/src/App.vue.tsx";
    let receipt = PendingProviderReady::authorize(
        &binding,
        5,
        0,
        "tsgo",
        &[ide_companion(
            ide_uri,
            "export const __IDE_V1 = 1;\n",
            Some(r#"{"version":3,"mappings":"AAAA"}"#),
            5,
        )],
    )
    .confirm_opened(&[ProviderPathKind::Ide]);

    let state = owned_carrier_state(); // ide_path == ide_uri
    commit_carrier_provider_state(&states, "/workspace/src/App.vue", state, &receipt);

    let committed = states
        .get("/workspace/src/App.vue")
        .expect("the receipt-gated commit writes the carrier state");
    let stamp = committed
        .committed_ide_surface
        .clone()
        .expect("the receipt-gated commit must stamp the committed IDE-surface identity");

    // The stamp is DERIVED FROM THE RECEIPT (not arbitrary): it equals the receipt's
    // CarrierIde companion fingerprint identity.
    let fingerprint = receipt
        .companions()
        .iter()
        .find(|c| c.role == verter_session::external_ts::SnapshotRole::CarrierIde)
        .expect("the receipt attests the IDE companion");
    assert_eq!(stamp.content_hash, fingerprint.content_hash);
    assert_eq!(stamp.map_hash, fingerprint.map_hash);

    // The committed (published) IDE surface CAN be captured.
    assert!(
        committed.authorizes_carrier_ide_capture(stamp.content_hash, stamp.map_hash),
        "the committed (published) IDE surface must be capturable"
    );
    // A surface whose CONTENT differs from the committed stamp (the failed-publish
    // window: a newer surface recorded but never committed) must NOT be capturable.
    assert!(
        !committed.authorizes_carrier_ide_capture([9u8; 16], stamp.map_hash),
        "a surface whose content differs from the committed stamp must be refused (fail closed)"
    );
    // A surface whose source-MAP differs (a map-only re-sync that never committed) must
    // also be refused — a result mapped through the superseded map would be wrong.
    assert!(
        !committed.authorizes_carrier_ide_capture(stamp.content_hash, [9u8; 16]),
        "a surface whose source-map differs from the committed stamp must be refused"
    );
}

#[tokio::test]
async fn direct_open_receipt_attests_the_exact_provider_specialized_ide_surface() {
    let owner = tempfile::tempdir().expect("temporary Svelte owner");
    let svelte_dir = owner.path().join("node_modules/svelte");
    std::fs::create_dir_all(&svelte_dir).expect("Svelte package directory");
    std::fs::write(
        svelte_dir.join("package.json"),
        r#"{"name":"svelte","version":"5.0.0","types":"./index.d.ts","exports":{".":{"types":"./index.d.ts"},"./elements":{"types":"./elements.d.ts"}}}"#,
    )
    .expect("Svelte package manifest");
    std::fs::write(
        svelte_dir.join("index.d.ts"),
        "export type Snippet = () => unknown;\n",
    )
    .expect("Svelte root types");
    std::fs::write(
        svelte_dir.join("elements.d.ts"),
        "export interface SvelteHTMLElements { div: {}; }\n",
    )
    .expect("Svelte element types");
    let ide_path = owner.path().join("src/App.svelte.tsx");
    std::fs::create_dir_all(ide_path.parent().expect("IDE parent")).expect("IDE parent directory");
    let ide_path = ide_path.to_string_lossy().into_owned();
    let compiler_surface = "/** @jsxImportSource @verter/svelte-jsx */\nconst view = <div />;\n";

    let provider = MockTypeProvider::new();
    let sync = crate::type_provider::project_sync::ProjectSync::new_with_kind(
        Arc::new(provider.clone()),
        crate::ProjectSyncMode::FullProject,
        crate::TypeProviderKind::Tsgo,
    );
    sync.open_tsx(&ide_path, compiler_surface)
        .await
        .expect("provider open succeeds");
    let exact_surface = sync
        .synced_tsx_surface(&ide_path)
        .expect("successful open mints exact surface evidence");
    assert_ne!(
        exact_surface.content().as_ref(),
        compiler_surface,
        "the test must exercise provider specialization, not byte-identical Vue behavior"
    );

    let binding = test_binding("/workspace/tsconfig.json");
    let receipt = PendingProviderReady::authorize(
        &binding,
        7,
        0,
        "tsgo",
        &[ide_companion(&ide_path, compiler_surface, None, 7)],
    )
    .confirm_opened_with_ide_surface(&[ProviderPathKind::Ide], Some(exact_surface.clone()));
    let fingerprint = receipt
        .companions()
        .iter()
        .find(|companion| companion.role == verter_session::external_ts::SnapshotRole::CarrierIde)
        .expect("IDE companion is attested");
    let exact_hash =
        crate::provider_surface_store::ContentHash::of(exact_surface.content()).to_hash16();
    let compiler_hash =
        crate::provider_surface_store::ContentHash::of(compiler_surface).to_hash16();
    assert_eq!(fingerprint.content_hash, exact_hash);
    assert_ne!(fingerprint.content_hash, compiler_hash);

    let states = DashMap::new();
    let state = ProviderSyncState {
        owner_binding: ProviderOwnerBinding::Owned("/workspace/tsconfig.json".to_owned()),
        ide_path: Some(ide_path),
        ide_background_loaded: true,
        ..Default::default()
    };
    commit_carrier_provider_state(&states, "/workspace/src/App.svelte", state, &receipt);
    let committed = states
        .get("/workspace/src/App.svelte")
        .expect("specialized carrier state commits");
    assert!(
        committed.authorizes_carrier_ide_capture(exact_hash, [0; 16]),
        "the committed receipt authorizes the exact provider/store surface"
    );
    assert!(
        !committed.authorizes_carrier_ide_capture(compiler_hash, [0; 16]),
        "the pre-adaptation compiler surface is not misreported as provider authority"
    );
}

#[test]
fn unresolved_state_authorizes_any_ide_surface_without_a_stamp() {
    // An UNRESOLVED (editor-liveness) carrier records its IDE surface only AFTER a
    // successful direct sync and carries NO membership stamp, so any live surface for it
    // is capturable. This is the pass-through the fail-closed OWNED gate must NOT break.
    let mut state = owned_carrier_state();
    state.owner_binding = ProviderOwnerBinding::Unresolved;
    state.committed_ide_surface = None;
    assert!(
        state.authorizes_carrier_ide_capture([1u8; 16], [2u8; 16]),
        "an unresolved carrier has no membership stamp and authorizes any live IDE surface"
    );
}

#[test]
fn owned_state_without_a_committed_stamp_authorizes_nothing() {
    // An OWNED carrier reaches the provider only through the receipt-gated commit; an
    // OWNED state that carries NO committed IDE stamp cannot authorize any capture (fail
    // closed — no captured surface ⇒ no mapping through uncommitted content).
    let mut state = owned_carrier_state();
    state.committed_ide_surface = None;
    assert!(
        !state.authorizes_carrier_ide_capture([1u8; 16], [2u8; 16]),
        "an owned state without a committed stamp must refuse every capture"
    );
}

#[test]
fn api_only_commit_preserves_prior_committed_ide_surface_stamp() {
    // A commit whose receipt attests NO IDE companion at the state's live ide_path (an
    // api-only refresh) must PRESERVE the prior committed IDE stamp — so the fail-closed
    // capture keeps rejecting a newer uncommitted surface even across an api-only pass.
    //
    // DISCRIMINATING: clearing the stamp on an IDE-less commit would leave the OWNED
    // state stampless (`s2 == None`), so the `.expect` below would panic — re-opening
    // the failed-publish window after any api refresh.
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let binding = test_binding("/workspace/tsconfig.json");
    let ide_uri = "/workspace/src/App.vue.tsx";

    // A full publish stamps the IDE surface (S1).
    let receipt1 = PendingProviderReady::authorize(
        &binding,
        1,
        0,
        "tsgo",
        &[ide_companion(ide_uri, "IDE V1\n", None, 1)],
    )
    .confirm_opened(&[ProviderPathKind::Ide]);
    commit_carrier_provider_state(
        &states,
        "/workspace/src/App.vue",
        owned_carrier_state(),
        &receipt1,
    );
    let s1 = states
        .get("/workspace/src/App.vue")
        .unwrap()
        .committed_ide_surface
        .clone()
        .expect("the first (full) commit stamps the IDE surface");

    // An api-only refresh: the receipt attests only an API companion (no IDE companion).
    let api_companion = CarrierCompanion {
        provider_uri: Arc::from("/workspace/src/App.vue.verter.ts"),
        content: Arc::from("API V2\n"),
        map_json: None,
        role: verter_session::external_ts::SnapshotRole::CarrierApi,
        script_kind: verter_session::external_ts::ScriptKind::Ts,
        version: 2,
    };
    let receipt2 = PendingProviderReady::authorize(&binding, 2, 0, "tsgo", &[api_companion])
        .confirm_opened(&[ProviderPathKind::Api]);
    commit_carrier_provider_state(
        &states,
        "/workspace/src/App.vue",
        owned_carrier_state(),
        &receipt2,
    );

    let s2 = states
        .get("/workspace/src/App.vue")
        .unwrap()
        .committed_ide_surface
        .clone()
        .expect("an api-only commit must PRESERVE the prior committed IDE-surface stamp");
    assert_eq!(
        s1, s2,
        "an api-only commit (same live ide_path) must preserve the prior IDE-surface stamp"
    );
}

/// A resolved `ProjectBinding` at an EXPLICIT ownership generation — the compare-and-swap
/// discriminant the admission gate orders on alongside the source revision.
fn test_binding_gen(tsconfig: &str, generation: u64) -> ProjectBinding {
    let env_dims = EnvDims {
        parse_env_hash: [0u8; 16],
        resolve_env_hash: [0u8; 16],
        lib_env_hash: [0u8; 16],
        project_identity: ProjectIdentity([0u8; 16]),
    };
    ProjectBinding::new_for_test(
        "/workspace",
        tsconfig,
        "5.9.0",
        env_dims,
        Vec::new(),
        ProjectId(0),
        SnapshotGeneration(generation),
    )
}

/// A tsgo receipt owned by `tsconfig` at ownership `generation` + source `revision`,
/// attesting one `CarrierIde` companion carrying `ide_content` at `App.vue.tsx` (the
/// `owned_carrier_state` IDE path).
fn tsgo_ide_receipt(
    tsconfig: &str,
    generation: u64,
    revision: u64,
    ide_content: &str,
) -> ProviderReadyReceipt {
    PendingProviderReady::authorize(
        &test_binding_gen(tsconfig, generation),
        revision,
        0,
        "tsgo",
        &[ide_companion(
            "/workspace/src/App.vue.tsx",
            ide_content,
            None,
            revision,
        )],
    )
    .confirm_opened(&[ProviderPathKind::Ide])
}

/// The `CarrierIde` companion content hash a receipt attests (for asserting which pass's
/// surface actually won the commit).
fn attested_ide_content_hash(receipt: &ProviderReadyReceipt) -> [u8; 16] {
    receipt
        .companions()
        .iter()
        .find(|c| c.role == SnapshotRole::CarrierIde)
        .expect("the receipt attests an IDE companion")
        .content_hash
}

#[test]
fn commit_refuses_a_stale_same_owner_receipt_by_generation_and_revision() {
    // Finding 1 (the prepare-then-open race): the admission gate compare-and-swap validates
    // the receipt's (ownership_generation, source_revision) against the CURRENT committed
    // stamp and REFUSES a strictly-older receipt — even for the SAME owner. Interleaving:
    // T1 prepares/opens content A, T2 commits a newer content B, then T1's superseded commit
    // must NOT overwrite B's committed state with stale A.
    //
    // DISCRIMINATING: pre-fix the gate validated ONLY the owner binding, so a same-owner
    // stale receipt committed and overwrote the newer state — the assertions below (the
    // newer stamp + surface survive, the stale commit is refused) would fail.
    let tsconfig = "/workspace/tsconfig.json";
    let states: DashMap<String, ProviderSyncState> = DashMap::new();

    // T2 commits the NEWER transaction (same owner, higher revision) first.
    let newer = tsgo_ide_receipt(tsconfig, 5, 10, "IDE NEW\n");
    let newer_ide_hash = attested_ide_content_hash(&newer);
    commit_carrier_provider_state(
        &states,
        "/workspace/src/App.vue",
        owned_carrier_state(),
        &newer,
    );

    // T1's superseded commit (SAME owner, OLDER revision) must be REFUSED — no overwrite.
    let stale = tsgo_ide_receipt(tsconfig, 5, 3, "IDE STALE\n");
    commit_carrier_provider_state(
        &states,
        "/workspace/src/App.vue",
        owned_carrier_state(),
        &stale,
    );

    let committed = states.get("/workspace/src/App.vue").unwrap();
    assert_eq!(
        committed.commit_stamp,
        Some(CarrierCommitStamp {
            ownership_generation: SnapshotGeneration(5),
            source_revision: 10,
        }),
        "the stale same-owner commit must be refused; the newer stamp \
         (higher generation, higher source counter) survives"
    );
    assert_eq!(
        committed
            .committed_ide_surface
            .as_ref()
            .unwrap()
            .content_hash,
        newer_ide_hash,
        "the newer IDE surface must survive the stale commit (stale content never installed)"
    );
    drop(committed);

    // The GENERATION dimension dominates: an older-generation receipt is refused even at a
    // much HIGHER revision.
    let older_gen = tsgo_ide_receipt(tsconfig, 4, 999, "IDE OLDGEN\n");
    commit_carrier_provider_state(
        &states,
        "/workspace/src/App.vue",
        owned_carrier_state(),
        &older_gen,
    );
    assert_eq!(
        states
            .get("/workspace/src/App.vue")
            .unwrap()
            .commit_stamp
            .unwrap()
            .ownership_generation,
        SnapshotGeneration(5),
        "an older-generation receipt is refused even at a higher revision (generation dominates)"
    );
}

#[test]
fn partial_open_api_ok_ide_fail_preserves_prior_live_ide_stamp_and_capture() {
    // Finding 3 (partial-open over-attestation): a tsgo direct open is PER-KIND. When the
    // API buffer opens but the IDE buffer FAILS, the receipt must attest ONLY the API
    // companion — so the gate PRESERVES the prior live IDE stamp instead of installing the
    // never-opened new IDE surface, and the still-live prior IDE surface stays capturable.
    //
    // DISCRIMINATING: pre-fix `confirm_opened` attested the COMPLETE companion set, so the
    // (unopened) new IDE companion stamped the state — replacing the live stamp (s1 != s2)
    // and rejecting the still-live prior IDE surface.
    let tsconfig = "/workspace/tsconfig.json";
    let ide_uri = "/workspace/src/App.vue.tsx";
    let states: DashMap<String, ProviderSyncState> = DashMap::new();

    // 1. A full publish opened + stamped the first IDE surface (the lower source counter).
    let r1 = tsgo_ide_receipt(tsconfig, 5, 1, "IDE V1\n");
    commit_carrier_provider_state(
        &states,
        "/workspace/src/App.vue",
        owned_carrier_state(),
        &r1,
    );
    let s1 = states
        .get("/workspace/src/App.vue")
        .unwrap()
        .committed_ide_surface
        .clone()
        .expect("the full publish stamps the IDE surface");

    // 2. A NEWER pass (the higher source counter): the API buffer opened, the IDE buffer FAILED. The pending
    //    carries BOTH a new IDE companion (V2) and an API companion, but only the API opened,
    //    so `confirm_opened(&[Api])` attests ONLY the API companion.
    let api_companion = CarrierCompanion {
        provider_uri: Arc::from("/workspace/src/App.vue.verter.ts"),
        content: Arc::from("API V2\n"),
        map_json: None,
        role: SnapshotRole::CarrierApi,
        script_kind: verter_session::external_ts::ScriptKind::Ts,
        version: 2,
    };
    let r2 = PendingProviderReady::authorize(
        &test_binding_gen(tsconfig, 5),
        2,
        0,
        "tsgo",
        &[
            ide_companion(ide_uri, "IDE V2 (never opened)\n", None, 2),
            api_companion,
        ],
    )
    .confirm_opened(&[ProviderPathKind::Api]);
    // The IDE kind failed, so the live IDE path is unchanged (reverted to prior) — the
    // committed state carries the SAME ide_path (`owned_carrier_state`).
    commit_carrier_provider_state(
        &states,
        "/workspace/src/App.vue",
        owned_carrier_state(),
        &r2,
    );

    let committed = states.get("/workspace/src/App.vue").unwrap();
    let s2 = committed
        .committed_ide_surface
        .clone()
        .expect("the partial-open commit must PRESERVE the prior IDE stamp (not clear it)");
    assert_eq!(
        s1, s2,
        "the partial open (API ok, IDE failed) must NOT replace the prior live IDE stamp"
    );
    // The still-live PRIOR IDE surface (V1) stays capturable; a DIFFERENT (e.g. the unopened
    // V2) surface is refused, proving the preserved stamp still gates fail-closed.
    assert!(
        committed.authorizes_carrier_ide_capture(s1.content_hash, s1.map_hash),
        "the prior live IDE surface must stay capturable after the partial open"
    );
    assert!(
        !committed.authorizes_carrier_ide_capture([9u8; 16], s1.map_hash),
        "an IDE surface other than the preserved committed one must be refused (fail closed)"
    );
}

#[test]
fn terminal_retract_classification_gates_unresolved_on_success() {
    // A TERMINAL owner-loss (NoProject/Ambiguous) may report the terminal `Unresolved`
    // — which the call site treats as "retracted, clear local state and stop retrying" —
    // ONLY after the store tombstone SUCCEEDED. An ERRORED retract must classify as
    // `RetryPending` (the gateway returns `Pending`: preserve local state + retry), so a
    // failed cross-process retract never masquerades as a completed one.
    //
    // DISCRIMINATING: the pre-fix behaviour ("always Unresolved on a terminal, even when
    // the retract errored") corresponds to classifying `Err` as `Tombstoned`, which
    // flips the second assertion.
    let ok: Result<(), &str> = Ok(());
    let err: Result<(), &str> = Err("store retract failed");
    assert_eq!(
        classify_terminal_retract(&ok),
        TerminalRetractDecision::Tombstoned,
        "a successful tombstone authorizes the terminal Unresolved"
    );
    assert_eq!(
        classify_terminal_retract(&err),
        TerminalRetractDecision::RetryPending,
        "a failed retract must keep the carrier Pending (preserve local state + retry)"
    );
}

#[test]
fn pending_provider_ready_confirm_mints_receipt_attesting_the_binding() {
    // The tsgo direct-open path no longer mints in the gateway: it hands back a
    // `PendingProviderReady`, and the SOLE tsgo mint is `confirm_opened`, called by the
    // site AFTER its companion buffers open. The minted receipt must attest the SAME
    // resolved binding the pending authorized (so the downstream owner-binding check accepts it),
    // and carry the authorized source revision.
    let binding = test_binding("/workspace/tsconfig.json");
    let pending = PendingProviderReady::authorize(&binding, 3, 0, "tsgo", &[]);
    let receipt = pending.confirm_opened(&[]);
    assert_eq!(
        receipt.binding().tsconfig_uri(),
        "/workspace/tsconfig.json",
        "confirm_opened must mint a receipt attesting the authorized binding"
    );
    assert_eq!(
        receipt.source_revision(),
        3,
        "confirm_opened must carry the authorized source revision"
    );
}

#[test]
fn carrier_close_target_returns_companion_paths_owner_independent() {
    // The close-only path computes the carrier provider paths to close WITHOUT a
    // receipt (it is not a commit) and WITHOUT resolving ownership — a carrier's
    // buffers must be closable regardless of its ownership state (e.g. after an owner
    // loss). A carrier path yields both companion paths under an `Unresolved` binding;
    // a non-carrier path yields `None` (the single carrier-vs-not gate).
    let resolver = crate::project_resolver::NativeProjectResolver::new(vec![
        crate::project_resolver::IdeProjectConfig::new(
            "/workspace".to_string(),
            "/workspace".to_string(),
            Some("/workspace/tsconfig.json".to_string()),
        ),
    ]);

    let target = carrier_close_target(&resolver, "/workspace/src/App.vue", false, None)
        .expect("a carrier has provider paths to close");
    assert_eq!(
        target.owner_binding,
        ProviderOwnerBinding::Unresolved,
        "the close target is owner-independent — it resolves no ownership"
    );
    assert!(
        target.ide_path.is_some() && target.api_path.is_some(),
        "the close target carries both companion paths: {target:?}"
    );

    assert!(
        carrier_close_target(&resolver, "/workspace/src/plain.ts", false, None).is_none(),
        "a non-carrier path has no IDE companion ⇒ no close target"
    );
}

/// A unique, already-canonical (lowercase drive, forward slashes) workspace root, so
/// the on-disk carrier store dir is isolated per run.
fn unique_ws_root() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("d:/verter_carrier_sync_compilefail_{nanos}_{n}/ws")
}

/// A `WorkspaceSnapshot` with ONE configured project owning `src/**/*` (so a `.vue`
/// under `src/` resolves to a `ProjectBinding`), built through the production
/// membership/expansion chain over an in-memory workspace.
fn project_binding_snapshot(ws_root: &str, tsconfig: &str) -> WorkspaceSnapshot {
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![ws_root.to_string()],
        default_resolve_extensions: None,
    });
    ws.inject_file(
        tsconfig.to_string(),
        Arc::<str>::from(r#"{ "include": ["src/**/*"] }"#),
    );
    ws.inject_file(
        format!("{ws_root}/src/Comp.vue"),
        Arc::<str>::from("<template></template>"),
    );

    let root = CanonicalPath::new(ws_root);
    let raw_membership = load_project_membership(&ws, tsconfig);
    let compiler_options = load_compiler_options(&ws, tsconfig);
    let supported = supported_extensions_for(&compiler_options);
    let spec = membership_to_spec(&root, &raw_membership, &supported);
    let references = load_project_references(&ws, tsconfig)
        .into_iter()
        .map(|r| CanonicalPath::new(&r))
        .collect();
    let project = OwnershipProject {
        id: ProjectId(0),
        root: root.clone(),
        workspace_root: CanonicalPath::new(ws_root),
        payload: ProjectPayload::Configured {
            tsconfig_path: CanonicalPath::new(tsconfig),
            membership: ConfiguredMembership {
                spec,
                materialized_files: Default::default(),
            },
            compiler_options,
            references,
            workspace_aliases: Vec::new(),
        },
    };
    build_workspace_snapshot_simple(vec![project], SnapshotGeneration(1))
}

/// Whether `provider` is still in the project's `ready_files` set (the cross-process
/// advertised surface the plugin's `getExternalFiles` serves).
fn carrier_ready_in_store(ws_root: &str, tsconfig: &str, provider: &str) -> bool {
    let store = CarrierPublishStore::open(default_carrier_store_host_version(), ws_root);
    let manifest = store.current_manifest();
    manifest
        .projects
        .get(tsconfig)
        .is_some_and(|project| project.ready_files.contains_key(provider))
}

/// An owned carrier that PREVIOUSLY published, then compiles to an EMPTY companion
/// set (neither an IDE surface nor a public-API artifact), must be RETRACTED from the
/// on-disk store — its stale `ready_files` row must DISAPPEAR so the plugin stops
/// advertising it. This drives the production gateway entry
/// (`reconcile_carrier_source`) for the genuinely-empty owned case (the
/// `ReconcileReason::CompileFailed` production constructor). RED before the fix: the
/// empty-companions branch returned `Pending` WITHOUT retracting, so the prior
/// advertisement lingered indefinitely.
#[tokio::test]
async fn owned_carrier_compiling_to_empty_companions_retracts_stale_advertisement() {
    let ws_root = unique_ws_root();
    let tsconfig = format!("{ws_root}/tsconfig.json");
    let source = format!("{ws_root}/src/Comp.vue");
    let provider = format!("{ws_root}/src/Comp.vue.tsx");

    let mock = MockTypeProvider::new();
    let backend = Arc::new(TsserverEngineBackend::with_default_host_version());
    let coord =
        CarrierPublishCoordinator::new(Arc::clone(&backend), Arc::new(mock.clone()), "5.9.0");

    let vfs: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = VerterHost::new(HostConfig::default(), vfs);
    let fs =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    fs.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(
        project_binding_snapshot(&ws_root, &tsconfig),
    )));

    // 1. Publish the carrier under its configured owner (a non-empty companion set)
    //    through the single membership entry — it enters the store's `ready_files`.
    let companion = CarrierCompanion {
        provider_uri: Arc::from(provider.as_str()),
        content: Arc::from("export default {} as any;\n"),
        map_json: None,
        role: verter_session::external_ts::SnapshotRole::CarrierIde,
        script_kind: verter_session::external_ts::ScriptKind::Tsx,
        version: 1,
    };
    let published = coord
        .reconcile_membership(
            &host,
            &fs,
            &source,
            vec![companion],
            true,
            ReconcileReason::SourceSynced,
        )
        .await
        .expect("the initial publish under a configured owner succeeds");
    assert!(
        matches!(published, ReconcileOutcome::Advertised { .. }),
        "the initial publish resolves to a configured owner ⇒ advertised, got {published:?}"
    );
    assert!(
        carrier_ready_in_store(&ws_root, &tsconfig, &provider),
        "the carrier must be advertised in the store's ready_files after the initial publish"
    );

    // 2. The source now compiles to NOTHING (no IDE surface, no public-API artifact)
    //    while it STILL has an authoritative owner. The resolver resolves the owner,
    //    but the host yields no compiled artifacts (it was never compiled), so the
    //    gateway builds an EMPTY companion set under authoritative ownership.
    let resolver = NativeProjectResolver::new(vec![IdeProjectConfig::new(
        ws_root.clone(),
        ws_root.clone(),
        Some(tsconfig.clone()),
    )]);
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let surfaces = ProviderSurfaceStore::new();
    let admission = CarrierTransactionCoordinator::new();
    let decision = reconcile_carrier_source(CarrierSyncRequest {
        host: &host,
        vfs: Some(&fs),
        ownership_ready: true,
        resolver: &resolver,
        provider_sync_states: &states,
        provider_surfaces: &surfaces,
        documents: None,
        canonical_id: &source,
        is_jsx: false,
        ide: None,
        membership: Some(CarrierMembershipCtx {
            coordinator: &coord,
            provider_delivery: CarrierProviderDelivery::StoreBacked,
        }),
        admission: &admission,
        reason: ReconcileReason::SourceSynced,
    })
    .await;
    // The owned compile-to-empty pass advertises nothing this pass: a non-owned outcome
    // whose settle disposition is `Pending` (keep queued; nothing committed).
    let CarrierSyncDecision::NotOwned(not_owned) = decision else {
        panic!("an owned compile-to-empty pass advertises nothing this pass (NotOwned)");
    };
    assert_eq!(
        admission.settle(not_owned, &source, None),
        SettleClass::Pending,
        "an owned compile-to-empty pass settles as Pending (keep queued)"
    );

    // 3. The stale advertisement MUST be gone from the store — the compile-to-empty
    //    owned case retracts the previously-published carrier.
    assert!(
        !carrier_ready_in_store(&ws_root, &tsconfig, &provider),
        "an owned carrier that compiled to EMPTY companions MUST be retracted from the \
         store's ready_files (so the plugin stops advertising it); the stale row lingered"
    );
}

#[tokio::test]
async fn managed_tsgo_reconcile_publishes_editor_membership_and_keeps_direct_open() {
    let ws_root = unique_ws_root();
    let tsconfig = format!("{ws_root}/tsconfig.json");
    let source = format!("{ws_root}/src/Comp.vue");
    let vfs: Arc<dyn verter_workspace::WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions {
            roots: vec![ws_root.clone()],
            default_resolve_extensions: None,
        }));
    let host = VerterHost::new(HostConfig::default(), vfs);
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: source.clone(),
            source: Arc::from(
                "<script setup lang=\"ts\">defineProps<{ label: string }>()</script><template><div>{{ label }}</div></template>",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("load carrier");
    let profile = CompileProfile::default();
    let _ = host.ensure_ide_compiled(&source, &profile);
    let ide = host.get_ide(&source, &profile).expect("IDE projection");

    let fs =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    fs.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(
        project_binding_snapshot(&ws_root, &tsconfig),
    )));
    let resolver = NativeProjectResolver::new(vec![IdeProjectConfig::new(
        ws_root.clone(),
        ws_root.clone(),
        Some(tsconfig.clone()),
    )]);
    let backend = Arc::new(TsserverEngineBackend::with_default_host_version());
    let coordinator = CarrierPublishCoordinator::new_editor_owned(
        Arc::clone(&backend),
        default_carrier_store_host_version(),
    );
    let states = DashMap::new();
    let surfaces = ProviderSurfaceStore::new();
    let admission = CarrierTransactionCoordinator::new();

    let decision = reconcile_carrier_source(CarrierSyncRequest {
        host: &host,
        vfs: Some(&fs),
        ownership_ready: true,
        resolver: &resolver,
        provider_sync_states: &states,
        provider_surfaces: &surfaces,
        documents: None,
        canonical_id: &source,
        is_jsx: ide.is_jsx,
        ide: Some(&ide),
        membership: Some(CarrierMembershipCtx {
            coordinator: &coordinator,
            provider_delivery: CarrierProviderDelivery::DirectOpen,
        }),
        admission: &admission,
        reason: ReconcileReason::SourceSynced,
    })
    .await;

    let CarrierSyncDecision::DirectOpen { transition, .. } = decision else {
        panic!("managed tsgo must retain direct provider delivery after store publication");
    };
    let ide_path = transition.next.ide_path.expect("IDE path");
    let api_path = transition.next.api_path.expect("API path");
    assert!(carrier_ready_in_store(&ws_root, &tsconfig, &ide_path));
    assert!(carrier_ready_in_store(&ws_root, &tsconfig, &api_path));
    assert!(
        states.get(&source).is_none(),
        "provider state may commit only after the direct opens succeed"
    );
}

// ── Scanner tier == resolver ownership (byte-equivalent classification) ──

/// The directory containing a tsconfig path (its project root).
fn tsconfig_dir(tsconfig: &str) -> String {
    tsconfig
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_else(|| tsconfig.to_string())
}

/// Deterministic non-zero R21 env dims (a stand-in for the host's per-project
/// env-hash reader); the ownership decision under test never depends on their value.
fn test_env_dims(_tsconfig_uri: &str) -> EnvDims {
    EnvDims {
        parse_env_hash: [1u8; 16],
        resolve_env_hash: [2u8; 16],
        lib_env_hash: [3u8; 16],
        project_identity: ProjectIdentity([4u8; 16]),
    }
}

/// Build an in-memory workspace + a real `WorkspaceSnapshot` with one configured
/// project per `(tsconfig_path, include_json_array)` entry, plus the given carrier
/// SOURCES injected. NO companion files are injected, so the carrier-path conflict
/// pass stays inert and OWNERSHIP alone drives the resolution. Returns `(ws, snap)`.
fn ws_and_snapshot(
    ws_root: &str,
    projects: &[(&str, &str)],
    sources: &[&str],
) -> (MemoryWorkspace, WorkspaceSnapshot) {
    let ws = MemoryWorkspace::new(MemoryOptions {
        roots: vec![ws_root.to_string()],
        default_resolve_extensions: None,
    });
    for (tsconfig, include) in projects {
        ws.inject_file(
            (*tsconfig).to_string(),
            Arc::<str>::from(format!(r#"{{ "include": {include} }}"#).as_str()),
        );
    }
    for src in sources {
        ws.inject_file(
            (*src).to_string(),
            Arc::<str>::from("<template></template>"),
        );
    }
    let mut ownership_projects = Vec::new();
    for (i, (tsconfig, _)) in projects.iter().enumerate() {
        let root = CanonicalPath::new(&tsconfig_dir(tsconfig));
        let raw_membership = load_project_membership(&ws, tsconfig);
        let compiler_options = load_compiler_options(&ws, tsconfig);
        let supported = supported_extensions_for(&compiler_options);
        let spec = membership_to_spec(&root, &raw_membership, &supported);
        let references = load_project_references(&ws, tsconfig)
            .into_iter()
            .map(|r| CanonicalPath::new(&r))
            .collect();
        ownership_projects.push(OwnershipProject {
            id: ProjectId(i as u32),
            root: root.clone(),
            workspace_root: CanonicalPath::new(ws_root),
            payload: ProjectPayload::Configured {
                tsconfig_path: CanonicalPath::new(tsconfig),
                membership: ConfiguredMembership {
                    spec,
                    materialized_files: Default::default(),
                },
                compiler_options,
                references,
                workspace_aliases: Vec::new(),
            },
        });
    }
    let snap = build_workspace_snapshot_simple(ownership_projects, SnapshotGeneration(1));
    (ws, snap)
}

#[test]
fn scanner_tier_and_resolver_ownership_are_byte_equivalent() {
    // The scanner's tier classifier (`classify_from_snapshot`) and the session
    // carrier resolver (`WorkspaceProjectResolver::resolve`) BOTH derive from the
    // SAME `WorkspaceSnapshot::configured_owner_resolution_for_file`, so they can
    // never disagree on whether a carrier has a configured owner. Byte-equivalent on
    // the OWNERSHIP axis: scanner `ProjectSource` <=> resolver
    // `Bound`/`Ambiguous(MultipleOwners)`; scanner `Other` <=> resolver `NoProject`.
    //
    // DISCRIMINATING: a scanner that reverted to glob patterns (`classify_tiers`), or
    // a resolver that COLLAPSED a multiply-owned carrier to a single `Bound`, would
    // break the agreement below.
    let ws_root = "d:/ws";
    let owned = "d:/ws/app/src/Owned.vue";
    let multi = "d:/ws/multi/src/Multi.vue";
    let orphan = "d:/ws/orphan/Orphan.vue";
    let (ws, snap) = ws_and_snapshot(
        ws_root,
        &[
            ("d:/ws/app/tsconfig.json", r#"["**/*"]"#),
            ("d:/ws/multi/tsconfig.json", r#"["**/*"]"#),
            ("d:/ws/multi/tsconfig.app.json", r#"["**/*"]"#),
        ],
        &[owned, multi, orphan],
    );
    let resolver = WorkspaceProjectResolver::new(
        &snap,
        &ws,
        "7.0.1",
        &(test_env_dims as fn(&str) -> EnvDims),
        true,
    );

    for (path, expect_tier) in [
        (owned, Tier::ProjectSource),
        (multi, Tier::ProjectSource),
        (orphan, Tier::Other),
    ] {
        let tier = classify_from_snapshot(&[path.to_string()], &snap)[0].1;
        let resolution = resolver.resolve(path, None);
        assert_eq!(tier, expect_tier, "scanner tier mismatch for {path}");
        // The invariant: scanner "is a project source" == resolver "has a configured
        // owner" (Bound OR MultipleOwners-ambiguous), computed for the SAME path from
        // the SAME snapshot.
        let scanner_owned = tier == Tier::ProjectSource;
        let resolver_owned = matches!(
            resolution,
            CarrierOwnershipResolution::Bound(_)
                | CarrierOwnershipResolution::Ambiguous {
                    cause: AmbiguityCause::MultipleOwners,
                    ..
                }
        );
        assert_eq!(
            scanner_owned, resolver_owned,
            "scanner tier and resolver ownership must agree for {path}: \
             tier={tier:?}, resolution={resolution:?}"
        );
    }

    // Anchor the equivalence to CONCRETE resolution states (so the agreement above is
    // not a vacuous tautology), and pin the non-collapsing 2-candidate ambiguity.
    assert!(
        matches!(
            resolver.resolve(owned, None),
            CarrierOwnershipResolution::Bound(_)
        ),
        "the uniquely-owned carrier resolves Bound"
    );
    match resolver.resolve(multi, None) {
        CarrierOwnershipResolution::Ambiguous { candidates, cause } => {
            assert_eq!(cause, AmbiguityCause::MultipleOwners);
            assert_eq!(
                candidates.len(),
                2,
                "both overlapping configs preserved (non-collapsing), got {candidates:?}"
            );
        }
        other => panic!(
            "the multiply-owned carrier must resolve Ambiguous(MultipleOwners), got {other:?}"
        ),
    }
    assert_eq!(
        resolver.resolve(orphan, None),
        CarrierOwnershipResolution::NoProject,
        "the unowned carrier resolves NoProject"
    );
}

// ── Ambiguity emits a verter(project) diagnostic AND registers no provider ──

#[test]
fn unresolved_carrier_emits_verter_project_diagnostic_bound_and_notready_silent() {
    use tower_lsp_server::ls_types::DiagnosticSeverity;

    // NoProject → a user-visible verter(project) WARNING naming the absent project.
    let d = project_ownership_diagnostic(&CarrierOwnershipResolution::NoProject)
        .expect("NoProject must surface a verter(project) diagnostic");
    assert_eq!(d.source.as_deref(), Some("verter(project)"));
    assert_eq!(d.severity, Some(DiagnosticSeverity::WARNING));
    assert!(
        d.message.contains("no configured"),
        "NoProject message: {}",
        d.message
    );

    // Ambiguous(MultipleOwners) → lists BOTH candidate configs.
    let d = project_ownership_diagnostic(&CarrierOwnershipResolution::Ambiguous {
        candidates: vec![
            Arc::from("d:/ws/a/tsconfig.json"),
            Arc::from("d:/ws/b/tsconfig.json"),
        ],
        cause: AmbiguityCause::MultipleOwners,
    })
    .expect("Ambiguous(MultipleOwners) must surface a diagnostic");
    assert_eq!(d.source.as_deref(), Some("verter(project)"));
    assert!(
        d.message.contains("a/tsconfig.json") && d.message.contains("b/tsconfig.json"),
        "the diagnostic must list BOTH candidate configs, got: {}",
        d.message
    );

    // Ambiguous(disk-layout conflict) → empty candidates → a generic message, still
    // sourced verter(project).
    let d = project_ownership_diagnostic(&CarrierOwnershipResolution::Ambiguous {
        candidates: Vec::new(),
        cause: AmbiguityCause::CarrierPathOccupiedByRealFile,
    })
    .expect("a disk-layout ambiguity must still surface a diagnostic");
    assert_eq!(d.source.as_deref(), Some("verter(project)"));

    // Bound / NotReady are NOT the user's problem → NO diagnostic.
    let binding = test_binding("d:/ws/tsconfig.json");
    assert!(
        project_ownership_diagnostic(&CarrierOwnershipResolution::Bound(binding)).is_none(),
        "a Bound carrier must NOT emit a verter(project) diagnostic"
    );
    assert!(
        project_ownership_diagnostic(&CarrierOwnershipResolution::NotReady).is_none(),
        "a transient NotReady carrier must NOT emit a diagnostic (not the user's fault)"
    );
}

#[tokio::test]
async fn ambiguous_carrier_sync_registers_and_queries_no_provider() {
    // An AMBIGUOUS carrier is TERMINAL (never served): the sync writes NO provider
    // sync state AND never opens/loads/updates/closes a companion buffer on the
    // provider. Driven through the real gateway entry with a live coordinator +
    // MockTypeProvider. Two sibling tsconfigs in the SAME dir both `include ["**/*"]`
    // ⇒ `src/Comp.vue` is multiply-owned ⇒ Ambiguous(MultipleOwners) ⇒ Unresolved.
    let ws_root = unique_ws_root();
    let tsconfig_a = format!("{ws_root}/tsconfig.json");
    let tsconfig_b = format!("{ws_root}/tsconfig.app.json");
    let source = format!("{ws_root}/src/Comp.vue");

    let mock = MockTypeProvider::new();
    let backend = Arc::new(TsserverEngineBackend::with_default_host_version());
    let coord =
        CarrierPublishCoordinator::new(Arc::clone(&backend), Arc::new(mock.clone()), "5.9.0");

    let vfs: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = VerterHost::new(HostConfig::default(), vfs);
    let fs =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    let (_ws, snap) = ws_and_snapshot(
        &ws_root,
        &[
            (tsconfig_a.as_str(), r#"["**/*"]"#),
            (tsconfig_b.as_str(), r#"["**/*"]"#),
        ],
        &[source.as_str()],
    );
    fs.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(snap)));

    let resolver = NativeProjectResolver::new(vec![
        IdeProjectConfig::new(ws_root.clone(), ws_root.clone(), Some(tsconfig_a.clone())),
        IdeProjectConfig::new(ws_root.clone(), ws_root.clone(), Some(tsconfig_b.clone())),
    ]);
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let surfaces = ProviderSurfaceStore::new();
    let admission = CarrierTransactionCoordinator::new();

    let decision = reconcile_carrier_source(CarrierSyncRequest {
        host: &host,
        vfs: Some(&fs),
        ownership_ready: true,
        resolver: &resolver,
        provider_sync_states: &states,
        provider_surfaces: &surfaces,
        documents: None,
        canonical_id: &source,
        is_jsx: false,
        ide: None,
        membership: Some(CarrierMembershipCtx {
            coordinator: &coord,
            provider_delivery: CarrierProviderDelivery::StoreBacked,
        }),
        admission: &admission,
        reason: ReconcileReason::SourceSynced,
    })
    .await;

    // An ambiguous carrier is terminal ⇒ a non-owned outcome whose settle disposition is
    // `Unresolved` (never served; the owner-loss barrier advances).
    let CarrierSyncDecision::NotOwned(not_owned) = decision else {
        panic!("an ambiguous carrier is terminal ⇒ NotOwned (never served)");
    };
    assert_eq!(
        admission.settle(not_owned, &source, None),
        SettleClass::Unresolved,
        "an ambiguous carrier settles as Unresolved (terminal)"
    );
    assert!(
        states.get(&source).is_none(),
        "an ambiguous carrier must register NO provider sync state"
    );
    assert!(
        mock.file_sync_calls().is_empty(),
        "an ambiguous carrier must never open/load/update/close a companion buffer on the \
         provider, got: {:?}",
        mock.file_sync_calls()
    );
}

// ── No readiness receipt precedes carrier publication ──

#[tokio::test]
async fn readiness_receipt_never_precedes_store_publication() {
    // C2 invariant: the readiness receipt is minted at the END of the ordered
    // apply_owned transaction — AFTER the store publish + ledger commit. So a
    // readiness receipt is REPRESENTABLE only on the `Advertised` outcome, and only
    // once the carrier is durably published. This test pins both halves:
    //   * a publish yields `Advertised { receipt }` AND the store ALREADY shows the
    //     carrier ready at that moment (readiness never precedes publication), and
    //   * a fail-closed (ambiguous) reconcile yields `Tombstoned` — an outcome that
    //     structurally carries NO receipt (readiness cannot exist without a publish).
    let ws_root = unique_ws_root();
    let tsconfig = format!("{ws_root}/tsconfig.json");
    let source = format!("{ws_root}/src/Comp.vue");
    let provider = format!("{ws_root}/src/Comp.vue.tsx");

    let mock = MockTypeProvider::new();
    let backend = Arc::new(TsserverEngineBackend::with_default_host_version());
    let coord =
        CarrierPublishCoordinator::new(Arc::clone(&backend), Arc::new(mock.clone()), "5.9.0");

    let vfs: Arc<dyn verter_workspace::WorkspaceAccess> = Arc::new(
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default()),
    );
    let host = VerterHost::new(HostConfig::default(), vfs);
    let fs =
        verter_workspace::FilesystemWorkspace::new(verter_workspace::FilesystemOptions::default());
    fs.publish_snapshot(PublishedRoot::new_vfs_only(Arc::new(
        project_binding_snapshot(&ws_root, &tsconfig),
    )));

    // Baseline: nothing advertised yet ⇒ no readiness could have been emitted.
    assert!(
        !carrier_ready_in_store(&ws_root, &tsconfig, &provider),
        "baseline: the carrier must not be advertised before any publish"
    );

    // Publish the owned carrier through the single membership entry with a non-empty
    // companion ⇒ Advertised (carries the readiness receipt).
    let companion = CarrierCompanion {
        provider_uri: Arc::from(provider.as_str()),
        content: Arc::from("export default {} as any;\n"),
        map_json: None,
        role: verter_session::external_ts::SnapshotRole::CarrierIde,
        script_kind: verter_session::external_ts::ScriptKind::Tsx,
        version: 7,
    };
    let published = coord
        .reconcile_membership(
            &host,
            &fs,
            &source,
            vec![companion],
            true,
            ReconcileReason::SourceSynced,
        )
        .await
        .expect("the owned publish succeeds");

    match published {
        ReconcileOutcome::Advertised { receipt, .. } => {
            // The receipt exists ONLY here, and the store is ALREADY showing the
            // carrier ready — publication has happened by the time readiness is
            // representable (never receipt-first).
            assert!(
                carrier_ready_in_store(&ws_root, &tsconfig, &provider),
                "the store must show the carrier published at the moment the readiness \
                 receipt exists (readiness never precedes publication)"
            );
            // The receipt attests the SAME source revision as the published companion.
            assert_eq!(
                receipt.source_revision(),
                7,
                "the receipt attests the published companion's version"
            );
        }
        other => panic!("an owned publish must be Advertised (with a receipt), got {other:?}"),
    }

    // A fail-closed (ambiguous) reconcile yields a `Tombstoned` outcome — which has NO
    // receipt field at all: readiness is structurally unrepresentable without a publish.
    let ambiguous_source = format!("{ws_root}/src/Other.vue");
    let tombstoned = coord
        .reconcile_membership_with_resolution(
            &ambiguous_source,
            CarrierOwnershipResolution::Ambiguous {
                candidates: Vec::new(),
                cause: AmbiguityCause::MultipleOwners,
            },
            Vec::new(),
            ReconcileReason::SourceSynced,
        )
        .await
        .expect("an ambiguous reconcile tombstones");
    assert!(
        matches!(tombstoned, ReconcileOutcome::Tombstoned { .. }),
        "an ambiguous carrier yields a Tombstoned outcome that carries NO readiness receipt, \
         got {tombstoned:?}"
    );
}

// ---------------------------------------------------------------------------
// The per-source carrier transaction coordinator: F1 (equal-key artifact), F5
// (vacant-resurrection barrier), F3/F4 (dropped-outcome settle) discriminators.
// ---------------------------------------------------------------------------

#[test]
fn admit_refuses_equal_key_commit_carrying_a_different_artifact() {
    // F1: at an EQUAL (generation, revision) the admission gate is idempotent ONLY for the
    // IDENTICAL artifact. A commit carrying the committed generation/revision but a
    // DIFFERENT receipt-attested IDE surface (a torn/superseded production sharing a
    // revision) must be REFUSED, never overwriting the committed surface.
    //
    // DISCRIMINATING: the pre-fix strictly-older-only compare-and-swap admits any
    // equal-or-newer receipt, so the second (different) artifact overwrites the committed
    // surface, flipping both the outcome and the surface assertion.
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let coord = CarrierTransactionCoordinator::new();
    let source = "/workspace/src/App.vue";
    let ide_uri = "/workspace/src/App.vue.tsx";
    let tsconfig = "/workspace/tsconfig.json";

    // T1 commits artifact A at generation 1 with the source counter at 5.
    let r1 = PendingProviderReady::authorize(
        &test_binding_gen(tsconfig, 1),
        5,
        0,
        "tsgo",
        &[ide_companion(ide_uri, "IDE ARTIFACT A\n", None, 5)],
    )
    .confirm_opened(&[ProviderPathKind::Ide]);
    assert_eq!(
        coord.admit_owned(&states, source, owned_carrier_state(), &r1),
        AdmitOutcome::Admitted,
        "the first commit installs artifact A"
    );
    let surface_a = states
        .get(source)
        .expect("state committed")
        .committed_ide_surface
        .clone()
        .expect("artifact A stamped");

    // T2 carries the SAME generation 1 and source counter 5 but a DIFFERENT artifact B.
    let r2 = PendingProviderReady::authorize(
        &test_binding_gen(tsconfig, 1),
        5,
        0,
        "tsgo",
        &[ide_companion(
            ide_uri,
            "IDE ARTIFACT B (different)\n",
            None,
            5,
        )],
    )
    .confirm_opened(&[ProviderPathKind::Ide]);
    assert_eq!(
        coord.admit_owned(&states, source, owned_carrier_state(), &r2),
        AdmitOutcome::Superseded,
        "an equal-key commit carrying a DIFFERENT artifact must be REFUSED (idempotent only \
         for the identical surface)"
    );
    assert_eq!(
        states
            .get(source)
            .expect("state still present")
            .committed_ide_surface
            .clone(),
        Some(surface_a),
        "the committed surface stays artifact A, the equal-key different artifact never \
         overwrites it (the F1 stale-overwrite hole)"
    );
}

#[test]
fn admit_idempotent_for_the_identical_artifact_at_an_equal_key() {
    // The negative companion to F1: re-committing the IDENTICAL artifact at the same
    // (generation, revision) is admitted (idempotent). The equal-key gate refuses only a
    // DIFFERENT artifact, never a duplicate of the committed one.
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let coord = CarrierTransactionCoordinator::new();
    let source = "/workspace/src/App.vue";
    let ide_uri = "/workspace/src/App.vue.tsx";
    let make = || {
        PendingProviderReady::authorize(
            &test_binding_gen("/workspace/tsconfig.json", 1),
            5,
            0,
            "tsgo",
            &[ide_companion(ide_uri, "IDENTICAL IDE\n", None, 5)],
        )
        .confirm_opened(&[ProviderPathKind::Ide])
    };
    assert_eq!(
        coord.admit_owned(&states, source, owned_carrier_state(), &make()),
        AdmitOutcome::Admitted
    );
    assert_eq!(
        coord.admit_owned(&states, source, owned_carrier_state(), &make()),
        AdmitOutcome::Admitted,
        "re-committing the identical artifact at the equal key is idempotent"
    );
}

#[test]
fn admit_refuses_a_late_token_after_owner_loss_advanced_the_barrier() {
    // F5: a late tsgo pending, authorized under the old owner BEFORE an owner-loss, must be
    // REFUSED once the owner-loss advanced the barrier, EVEN into a vacant/unstamped slot.
    //
    // DISCRIMINATING: the barrier lives OUTSIDE the removable `ProviderSyncState`. A gate
    // that only compared against the (now-removed) `commit_stamp` sees a vacant slot with no
    // watermark and admits the obsolete owned receipt, resurrecting the old owner.
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let coord = CarrierTransactionCoordinator::new();
    let source = "/workspace/src/App.vue";
    let ide_uri = "/workspace/src/App.vue.tsx";
    let tsconfig = "/workspace/tsconfig.json";

    // The tsgo pending captures the CURRENT barrier epoch (0), before the loss.
    let epoch_at_authorize = coord.current_intent_epoch(source);
    let late = PendingProviderReady::authorize(
        &test_binding_gen(tsconfig, 1),
        5,
        epoch_at_authorize,
        "tsgo",
        &[ide_companion(ide_uri, "OBSOLETE OWNED IDE\n", None, 5)],
    )
    .confirm_opened(&[ProviderPathKind::Ide]);

    // An owner-loss / removal advances the barrier while the slot is vacant.
    coord.advance_barrier(source);

    // The late token commits into a VACANT slot; it must be REFUSED (no resurrection).
    assert_eq!(
        coord.admit_owned(&states, source, owned_carrier_state(), &late),
        AdmitOutcome::Superseded,
        "a token captured before an owner-loss must be refused even into a vacant slot"
    );
    assert!(
        states.get(source).is_none(),
        "the obsolete owned receipt must NOT resurrect the old owner into the vacated slot"
    );

    // Positive control: a fresh token authorized AT the current barrier admits normally.
    let fresh = PendingProviderReady::authorize(
        &test_binding_gen(tsconfig, 2),
        6,
        coord.current_intent_epoch(source),
        "tsgo",
        &[ide_companion(ide_uri, "FRESH OWNED IDE\n", None, 6)],
    )
    .confirm_opened(&[ProviderPathKind::Ide]);
    assert_eq!(
        coord.admit_owned(&states, source, owned_carrier_state(), &fresh),
        AdmitOutcome::Admitted,
        "a token authorized at the CURRENT barrier admits normally after the loss"
    );
}

#[test]
fn settle_finalizes_the_non_owned_disposition_no_dropped_outcome() {
    // F3/F4 (dropped-outcome class): the non-owned disposition is OWNED by the coordinator.
    // The opaque `CarrierNotOwned` (must_use + a private reason) can neither be dropped nor
    // routed by a caller, so the requeue (Pending / NotReady) and the owner-loss barrier
    // advance (terminal Unresolved) can never be silently lost.
    let coord = CarrierTransactionCoordinator::new();
    let requeue: dashmap::DashSet<String> = dashmap::DashSet::new();
    let source = "/workspace/src/App.vue";

    // A Pending (a failed retract / advertise miss) is REQUEUED, never dropped.
    assert_eq!(
        coord.settle(CarrierNotOwned::pending(), source, Some(&requeue)),
        SettleClass::Pending
    );
    assert!(
        requeue.contains(source),
        "a Pending non-owned outcome must be REQUEUED through the coordinator, never dropped"
    );

    // A transient NotReady is REQUEUED (the sole retryable owner-loss state).
    let requeue2: dashmap::DashSet<String> = dashmap::DashSet::new();
    assert_eq!(
        coord.settle(CarrierNotOwned::not_ready(), source, Some(&requeue2)),
        SettleClass::NotReady
    );
    assert!(
        requeue2.contains(source),
        "a NotReady non-owned outcome must be REQUEUED"
    );

    // A terminal Unresolved ALWAYS advances the owner-loss barrier (the F5 tombstone). The
    // background drain passes `None` and does NOT requeue it (it dequeues via
    // `SyncOutcome::Terminal`); an interactive caller passes a pending set and keeps an OPEN
    // unowned document queued for a future owner reconciliation.
    let before = coord.current_intent_epoch(source);
    assert_eq!(
        coord.settle(CarrierNotOwned::unresolved(), source, None),
        SettleClass::Unresolved
    );
    assert!(
        coord.current_intent_epoch(source) > before,
        "a terminal owner-loss must advance the barrier (the F5 tombstone)"
    );

    let requeue3: dashmap::DashSet<String> = dashmap::DashSet::new();
    let before2 = coord.current_intent_epoch(source);
    assert_eq!(
        coord.settle(CarrierNotOwned::unresolved(), source, Some(&requeue3)),
        SettleClass::Unresolved
    );
    assert!(
        coord.current_intent_epoch(source) > before2,
        "every terminal owner-loss advances the barrier"
    );
    assert!(
        requeue3.contains(source),
        "an interactive terminal Unresolved keeps an OPEN unowned document queued"
    );
}

#[test]
fn admit_admits_equal_key_path_rebind_as_a_legitimate_flip() {
    // A same-(generation, source counter) commit at a DIFFERENT IDE path is a legitimate REBIND
    // (a jsx↔tsx flip whose source counter is content-decoupled — no published vfs), NOT a torn
    // production: a torn production shares BOTH the source counter AND the path (same content ⇒
    // same is_jsx ⇒ same path). The equal-key idempotency refusal therefore applies ONLY to a
    // SAME-PATH different-artifact production; a differing path admits and rebinds. This locks
    // the coordinator's "a genuine path change still admits" design and the
    // `current_file_sync_reopens_when_live_ide_path_changes` invariant. (The reverted "refuse
    // equal-key different-path" tightening is a design fork — it provides no benefit in the
    // reliable-counter case, where a flip advances the counter and never reaches an equal key,
    // and drops a live path change in the content-decoupled case — tracked as a follow-up.)
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let coord = CarrierTransactionCoordinator::new();
    let source = "/workspace/src/App.vue";
    let tsconfig = "/workspace/tsconfig.json";

    // T1 commits the `.tsx` artifact at generation 1 / source counter 5.
    let tsx_uri = "/workspace/src/App.vue.tsx";
    let r1 = PendingProviderReady::authorize(
        &test_binding_gen(tsconfig, 1),
        5,
        0,
        "tsgo",
        &[ide_companion(tsx_uri, "TSX ARTIFACT\n", None, 5)],
    )
    .confirm_opened(&[ProviderPathKind::Ide]);
    assert_eq!(
        coord.admit_owned(&states, source, owned_carrier_state(), &r1),
        AdmitOutcome::Admitted,
    );

    // T2 shares the SAME generation 1 / source counter 5 but rebinds to a DIFFERENT IDE path
    // (`App.vue.jsx`) — a legitimate flip under a content-decoupled revision. It ADMITS.
    let jsx_uri = "/workspace/src/App.vue.jsx";
    let mut jsx_state = owned_carrier_state();
    jsx_state.ide_path = Some(jsx_uri.to_string());
    let r2 = PendingProviderReady::authorize(
        &test_binding_gen(tsconfig, 1),
        5,
        0,
        "tsgo",
        &[ide_companion(jsx_uri, "JSX ARTIFACT\n", None, 5)],
    )
    .confirm_opened(&[ProviderPathKind::Ide]);
    assert_eq!(
        coord.admit_owned(&states, source, jsx_state, &r2),
        AdmitOutcome::Admitted,
        "a same-key commit at a DIFFERENT IDE path is a legitimate rebind and must ADMIT"
    );
    assert_eq!(
        states.get(source).expect("state present").ide_path.clone(),
        Some(jsx_uri.to_string()),
        "the legitimate rebind switches the committed IDE path to .jsx"
    );
}

#[test]
fn admit_admits_a_jsx_tsx_flip_that_advances_the_revision() {
    // A GENUINE jsx↔tsx flip is driven by a source edit that ADVANCES the per-source revision
    // (`notify_upsert` → `bump_content_generation_for`), so it admits through the strictly-newer
    // path and rebinds the committed IDE path.
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let coord = CarrierTransactionCoordinator::new();
    let source = "/workspace/src/App.vue";
    let tsconfig = "/workspace/tsconfig.json";

    // T1 commits `.tsx` at generation 1 / source counter 5.
    let tsx_uri = "/workspace/src/App.vue.tsx";
    let r1 = PendingProviderReady::authorize(
        &test_binding_gen(tsconfig, 1),
        5,
        0,
        "tsgo",
        &[ide_companion(tsx_uri, "TSX ARTIFACT\n", None, 5)],
    )
    .confirm_opened(&[ProviderPathKind::Ide]);
    assert_eq!(
        coord.admit_owned(&states, source, owned_carrier_state(), &r1),
        AdmitOutcome::Admitted,
    );

    // T2 flips to `.jsx` with an ADVANCED source counter 6 (the edit that flipped the lang bumped the
    // per-source content revision): a strictly-newer commit that admits and rebinds the path.
    let jsx_uri = "/workspace/src/App.vue.jsx";
    let mut jsx_state = owned_carrier_state();
    jsx_state.ide_path = Some(jsx_uri.to_string());
    let r2 = PendingProviderReady::authorize(
        &test_binding_gen(tsconfig, 1),
        6,
        0,
        "tsgo",
        &[ide_companion(jsx_uri, "JSX ARTIFACT\n", None, 6)],
    )
    .confirm_opened(&[ProviderPathKind::Ide]);
    assert_eq!(
        coord.admit_owned(&states, source, jsx_state, &r2),
        AdmitOutcome::Admitted,
        "a jsx↔tsx flip that advances the revision admits through the strictly-newer path"
    );
    assert_eq!(
        states.get(source).expect("state present").ide_path.clone(),
        Some(jsx_uri.to_string()),
        "the genuine flip rebinds the committed IDE path to .jsx"
    );
}

#[test]
fn advance_barrier_and_remove_refuses_a_late_owner_token_into_the_vacated_slot() {
    // Advance-before-remove: removing a previously-committed carrier state through
    // the coordinator advances the owner-loss barrier BEFORE it vacates the slot, so a late
    // owned token captured before the removal is REFUSED even into the now-vacant slot.
    //
    // DISCRIMINATING: the pre-fix advance-AFTER-remove ordering vacated the slot first, so a
    // late token (old epoch) could admit into the vacated slot before the barrier advanced.
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let coord = CarrierTransactionCoordinator::new();
    let source = "/workspace/src/App.vue";
    let tsx_uri = "/workspace/src/App.vue.tsx";
    let tsconfig = "/workspace/tsconfig.json";

    // Commit a carrier state (stamps it), then capture a late token at the CURRENT epoch.
    let r1 = PendingProviderReady::authorize(
        &test_binding_gen(tsconfig, 1),
        5,
        coord.current_intent_epoch(source),
        "tsgo",
        &[ide_companion(tsx_uri, "IDE\n", None, 5)],
    )
    .confirm_opened(&[ProviderPathKind::Ide]);
    assert_eq!(
        coord.admit_owned(&states, source, owned_carrier_state(), &r1),
        AdmitOutcome::Admitted,
    );
    let late = PendingProviderReady::authorize(
        &test_binding_gen(tsconfig, 1),
        5,
        coord.current_intent_epoch(source),
        "tsgo",
        &[ide_companion(tsx_uri, "LATE IDE\n", None, 5)],
    )
    .confirm_opened(&[ProviderPathKind::Ide]);

    // Remove the committed carrier through the coordinator — advance-before-remove.
    let removed = coord.advance_barrier_and_remove(&states, source);
    assert!(removed.is_some(), "the committed carrier state is removed");
    assert!(states.get(source).is_none(), "the slot is vacated");

    // The late token (captured before the removal) is REFUSED into the vacated slot.
    assert_eq!(
        coord.admit_owned(&states, source, owned_carrier_state(), &late),
        AdmitOutcome::Superseded,
        "a token captured before the advance-before-remove must be refused into the vacated slot"
    );
    assert!(
        states.get(source).is_none(),
        "the obsolete owned receipt must NOT resurrect the removed owner"
    );
}

#[test]
fn advance_barrier_and_remove_does_not_advance_for_a_non_carrier_state() {
    // The negative companion: removing a state that was NEVER receipt-committed as a carrier (no
    // commit stamp) does NOT advance the barrier — only a previously-committed carrier removal is
    // an owner-loss.
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let coord = CarrierTransactionCoordinator::new();
    let source = "/workspace/src/App.vue";
    states.insert(
        source.to_string(),
        ProviderSyncState::unresolved("/workspace/src/App.vue.tsx".to_string()),
    );
    let before = coord.current_intent_epoch(source);
    let removed = coord.advance_barrier_and_remove(&states, source);
    assert!(removed.is_some());
    assert_eq!(
        coord.current_intent_epoch(source),
        before,
        "removing a non-carrier (unstamped) state must NOT advance the owner-loss barrier"
    );
}

#[test]
fn convert_to_unresolved_advances_barrier_clears_token_and_refuses_late_owner() {
    // Owned→unresolved conversion: converting a previously-committed OWNED carrier
    // to unresolved advances the owner-loss barrier AND clears the receipt-attested admission
    // token (`commit_stamp` / `committed_ide_surface`), so a late owned token captured before
    // the conversion is REFUSED.
    //
    // DISCRIMINATING: the pre-fix reuse-and-flip left the stale commit stamp on the converted
    // state and never advanced the barrier, so a late owned token could resurrect the owner.
    let states: DashMap<String, ProviderSyncState> = DashMap::new();
    let coord = CarrierTransactionCoordinator::new();
    let source = "/workspace/src/App.vue";
    let tsx_uri = "/workspace/src/App.vue.tsx";
    let tsconfig = "/workspace/tsconfig.json";

    // Commit an owned carrier, then capture a late token at the current epoch.
    let r1 = PendingProviderReady::authorize(
        &test_binding_gen(tsconfig, 1),
        5,
        coord.current_intent_epoch(source),
        "tsgo",
        &[ide_companion(tsx_uri, "IDE\n", None, 5)],
    )
    .confirm_opened(&[ProviderPathKind::Ide]);
    assert_eq!(
        coord.admit_owned(&states, source, owned_carrier_state(), &r1),
        AdmitOutcome::Admitted,
    );
    let late = PendingProviderReady::authorize(
        &test_binding_gen(tsconfig, 1),
        5,
        coord.current_intent_epoch(source),
        "tsgo",
        &[ide_companion(tsx_uri, "LATE IDE\n", None, 5)],
    )
    .confirm_opened(&[ProviderPathKind::Ide]);

    // Convert the reused owned state to unresolved through the coordinator.
    let mut reused = states.get(source).expect("committed").clone();
    let before = coord.current_intent_epoch(source);
    coord.convert_to_unresolved(source, &mut reused);
    assert!(
        coord.current_intent_epoch(source) > before,
        "converting a committed owned carrier advances the owner-loss barrier"
    );
    assert!(
        reused.commit_stamp.is_none() && reused.committed_ide_surface.is_none(),
        "the converted state carries NO receipt-attested admission token"
    );
    assert!(
        reused.owner_binding.is_unresolved(),
        "the converted state binding is forced Unresolved"
    );

    // A late owned token captured before the conversion is refused (the barrier advanced).
    assert_eq!(
        coord.admit_owned(&states, source, owned_carrier_state(), &late),
        AdmitOutcome::Superseded,
        "a late owned token captured before the owned→unresolved conversion is refused"
    );
}
