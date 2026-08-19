//! Content-mode compile publish fence: flight-captured identity stamps.
//!
//! The `Content`-mode compile publish keys the content-addressed entry
//! on the content hash plus the env-identity bundle (four split
//! env-dimension hashes + project identity) and stamps a
//! `validated_at_generation`. All of these MUST come from values the
//! flight captured BEFORE the compile — the content hash from the SAME
//! source snapshot that supplies the compiled bytes, never from
//! post-compile live re-reads. A live re-read forges currency: an env /
//! project mutation landing in the compute→publish window would publish
//! OLD-input bytes under the NEW-current identity, and a content
//! mutation landing between the snapshot and the compile-input assembly
//! would publish NEW-input bytes under the OLD content hash — an entry
//! the read-side rail can never reject because its key genuinely
//! matches the reverted content.
//!
//! Fence semantics (mirrors the `IndexedReady` pre-publish fence and
//! the overlay publish stamps): publish ONLY when the live identity —
//! content hash included — still equals the captured identity; on a
//! move, DECLINE the publish (ReturnOnly — the caller is still served
//! the freshly compiled output) and stamp nothing.

use std::sync::Arc;

use crate::types::{
    CompileCacheMode, CompileErrorPolicy, CompileProfile, FileLanguage, HostConfig, UpsertRequest,
    VirtualNodeKind, VirtualQuery,
};
use crate::VerterHost;
use verter_workspace::{
    IdeProjectCompilerOptions, MemoryOptions, MemoryWorkspace, ProjectGraph, ProjectRank,
    VfsProjectConfig,
};

const PROJECT_ROOT: &str = "/projN";
const CANONICAL: &str = "/projN/Plain.vue";

/// A fact-free SFC: no imports, no cross-file deps → no downgrade
/// reason fires → a `Content` request actually runs (and publishes) as
/// `Content`.
const FACT_FREE: &str =
    "<script setup lang=\"ts\">const n = 1</script><template><div>{{ n }}</div></template>";

fn project_config(paths: Vec<(String, Vec<String>)>) -> VfsProjectConfig {
    VfsProjectConfig {
        root: PROJECT_ROOT.to_string(),
        rank: ProjectRank::Explicit,
        tsconfig_path: Some(format!("{PROJECT_ROOT}/tsconfig.json")),
        root_files: vec![],
        extensions: vec![".ts".to_string(), ".vue".to_string()],
        workspace_root: PROJECT_ROOT.to_string(),
        workspace_aliases: vec![],
        compiler_options: IdeProjectCompilerOptions {
            base_url: Some(PROJECT_ROOT.to_string()),
            paths,
            ..Default::default()
        },
        references: vec![],
        membership: verter_workspace::ConfiguredMembership::match_all_under_root(
            &verter_workspace::CanonicalPath::new(PROJECT_ROOT),
        ),
    }
}

fn publish_graph(workspace: &MemoryWorkspace, paths: Vec<(String, Vec<String>)>) {
    workspace.set_project_graph(ProjectGraph::from_configs(vec![project_config(paths)]));
}

/// Production (non-dev) host over a mutable `MemoryWorkspace` so the
/// test can republish the project graph (→ new env-hash bundle) inside
/// the compute→publish window. The default `HostConfig` enables
/// `dev_mode` + `DevServeLastKnownGood`, which fires `HasDevLastGood`
/// on every compile and would downgrade every `Content` request to
/// `Stateless`.
fn host_over(workspace: Arc<MemoryWorkspace>) -> Arc<VerterHost> {
    Arc::new(VerterHost::new(
        HostConfig {
            dev_mode: false,
            compile_error_policy: CompileErrorPolicy::StrictError,
            ..HostConfig::default()
        },
        workspace,
    ))
}

fn content_profile() -> CompileProfile {
    CompileProfile {
        requested_mode: CompileCacheMode::Content,
        ..CompileProfile::default()
    }
}

fn compile(host: &VerterHost, profile: &CompileProfile) -> crate::types::VirtualFileResponse {
    host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(CANONICAL.to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile.clone(),
    })
    .expect("compile must serve")
}

/// An env-identity mutation landing between compute and publish must
/// DECLINE the content-addressed publish — never stamp the old-input
/// output under the new-current identity.
///
/// Discrimination: pre-fence, the publish rebuilt the key from LIVE
/// `host_view_env_hashes_for` / `host_view_project_identity_for` reads
/// after the compile, so the mutation landed the artifact under the
/// post-mutation identity and the entry count was 1; the fenced publish
/// declines, leaving the store empty.
#[test]
fn env_mutation_between_compute_and_publish_declines_the_content_publish() {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    publish_graph(
        &workspace,
        vec![("@n/*".to_string(), vec!["./src/*".to_string()])],
    );
    let host = host_over(Arc::clone(&workspace));
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(CANONICAL.to_string()),
            input_id: CANONICAL.to_string(),
            source: FACT_FREE.into(),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert vue");

    // Sanity: the mutation we are about to land actually moves the
    // env-identity bundle for the canonical (otherwise the fence test
    // is vacuous).
    let pre_env = host.host_view_env_hashes_for(CANONICAL);

    // Land the project-graph republish (→ different resolve_env_hash
    // for the owning project) inside the compute→publish window, once.
    {
        let workspace = Arc::clone(&workspace);
        let fired = std::sync::atomic::AtomicBool::new(false);
        *host.compile_publish_seam_hook.lock() = Some(Arc::new(move || {
            if !fired.swap(true, std::sync::atomic::Ordering::SeqCst) {
                publish_graph(
                    &workspace,
                    vec![("@moved/*".to_string(), vec!["./moved/*".to_string()])],
                );
            }
        }));
    }

    let profile = content_profile();
    let raced = compile(&host, &profile);
    *host.compile_publish_seam_hook.lock() = None;

    // The raced request must really have run as Content (a downgrade
    // would bypass the content node and make the pin vacuous) and must
    // still be served its freshly compiled output (ReturnOnly).
    assert_eq!(
        raced.actual_mode,
        CompileCacheMode::Content,
        "fixture must classify to Content — the fence pin is otherwise vacuous",
    );
    assert!(!raced.cache_hit, "first compile must be cold");
    assert!(
        !raced.code.is_empty(),
        "the declined publish must still serve the freshly compiled output",
    );
    let post_env = host.host_view_env_hashes_for(CANONICAL);
    assert_ne!(
        pre_env.resolve_env_hash, post_env.resolve_env_hash,
        "the mid-flight republish must actually move the env identity — \
         fence pin is otherwise vacuous",
    );

    // THE PIN: the mutation landed between compute and publish, so the
    // publish must DECLINE. Pre-fence the publish rebuilt the key from
    // live post-mutation env reads and landed the old-input output
    // under the new-current identity (entry count 1).
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "an env mutation in the compute→publish window must decline the \
         content-addressed publish — old-input bytes must never be \
         stamped under the new-current identity",
    );

    // Negative control: with the env now stable, the next compile
    // publishes normally and the one after warm-hits it — the fence
    // must not suppress steady-state publication.
    let republished = compile(&host, &profile);
    assert_eq!(republished.actual_mode, CompileCacheMode::Content);
    assert!(!republished.cache_hit, "no entry exists yet — cold");
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        1,
        "a stable-env compile must publish exactly one content entry",
    );
    let warm = compile(&host, &profile);
    assert!(
        warm.cache_hit,
        "the stable-env entry must serve the next request warm",
    );
    assert_eq!(
        warm.code, republished.code,
        "warm hit must be byte-identical"
    );
}

/// Fact-free variant of [`FACT_FREE`] with byte-distinct script
/// content, so a compile of one version is distinguishable from a
/// compile of the other in the emitted output.
const FACT_FREE_V2: &str = "<script setup lang=\"ts\">const marker_v2 = 2</script><template><div>{{ marker_v2 }}</div></template>";

/// Byte-distinct first version carrying its own marker (the shared
/// [`FACT_FREE`] fixture has no unique identifier to assert on).
const FACT_FREE_V1: &str = "<script setup lang=\"ts\">const marker_v1 = 1</script><template><div>{{ marker_v1 }}</div></template>";

fn upsert(host: &VerterHost, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(CANONICAL.to_string()),
            input_id: CANONICAL.to_string(),
            source: source.into(),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert vue");
}

/// A content upsert landing between the request's source-snapshot
/// capture and the compile-input assembly must never publish output
/// compiled from the NEW bytes under the OLD content hash.
///
/// Discrimination: pre-fix the compile input re-read the scheduler
/// source independently of the snapshot that supplied the captured
/// content key, so the raced compile consumed the post-upsert bytes
/// while the key carried the pre-upsert `whole_hash` — and the publish
/// fence rebuilt its live key FROM the captured hash, structurally
/// excluding the content dimension from the live-vs-captured compare.
/// The poisoned entry (new bytes under the old content hash) then
/// warm-hit any later request whose content reverted to the old bytes.
/// Post-fix the compiled bytes and the key derive from ONE snapshot
/// and the fence re-reads the LIVE content hash, so the raced publish
/// DECLINES and the caller is served output attributable to the
/// request-start snapshot.
#[test]
fn content_mutation_between_snapshot_and_compile_input_never_publishes_under_the_stale_hash() {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    publish_graph(
        &workspace,
        vec![("@n/*".to_string(), vec!["./src/*".to_string()])],
    );
    let host = host_over(Arc::clone(&workspace));
    upsert(&host, FACT_FREE_V1);

    // Land the content upsert (→ different `whole_hash`) inside the
    // snapshot→compile-input window, once.
    {
        let hook_host = Arc::clone(&host);
        let fired = std::sync::atomic::AtomicBool::new(false);
        *host.compile_input_seam_hook.lock() = Some(Arc::new(move || {
            if !fired.swap(true, std::sync::atomic::Ordering::SeqCst) {
                upsert(&hook_host, FACT_FREE_V2);
            }
        }));
    }

    let profile = content_profile();
    let raced = compile(&host, &profile);
    *host.compile_input_seam_hook.lock() = None;

    assert_eq!(
        raced.actual_mode,
        CompileCacheMode::Content,
        "fixture must classify to Content — the fence pin is otherwise vacuous",
    );
    assert!(!raced.cache_hit, "first compile must be cold");
    // The served output must be attributable to the request-start
    // snapshot: the compile input and the content key derive from ONE
    // coherent snapshot, never a mid-request re-read.
    assert!(
        raced.code.contains("marker_v1"),
        "the raced compile must consume the request-start snapshot bytes",
    );
    assert!(
        !raced.code.contains("marker_v2"),
        "the raced compile must not consume mid-request re-read bytes",
    );
    // THE PIN: the content moved mid-flight, so the publish must
    // DECLINE — nothing may be stamped under either content hash.
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "a content mutation in the snapshot→publish window must decline \
         the content-addressed publish — bytes must never be stamped \
         under a content hash they were not compiled from",
    );

    // Recovery: content now stable at v2 — cold publish + warm hit.
    let v2_cold = compile(&host, &profile);
    assert!(!v2_cold.cache_hit, "no entry exists yet — cold");
    assert!(v2_cold.code.contains("marker_v2"));
    assert_eq!(host.compile_output_pure_content_entry_count(), 1);
    let v2_warm = compile(&host, &profile);
    assert!(v2_warm.cache_hit, "the stable v2 entry must serve warm");
    assert_eq!(
        v2_warm.code, v2_cold.code,
        "warm hit must be byte-identical"
    );

    // Poison probe: revert the content to v1. A poisoned entry (v2
    // bytes stamped under the v1 content hash) would warm-hit here and
    // serve v2 output for v1 content — an entry the read-side rail can
    // never reject because the key genuinely matches the live content.
    upsert(&host, FACT_FREE_V1);
    let reverted = compile(&host, &profile);
    assert!(
        !reverted.cache_hit,
        "the revert must MISS — no entry may exist under the v1 content hash",
    );
    assert!(reverted.code.contains("marker_v1"));
    assert!(
        !reverted.code.contains("marker_v2"),
        "v1 content must never be served output compiled from v2 bytes",
    );
    // The revert upsert moved `whole_hash`, which flushes the
    // canonical's prior content entries (the upsert-side boundedness
    // flush mirrors `invalidate_compile_slots`'s `remove_canonical`),
    // so exactly the freshly published v1 entry remains.
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        1,
        "exactly the freshly published v1 entry remains after the \
         revert upsert's per-canonical content flush",
    );
    let reverted_warm = compile(&host, &profile);
    assert!(reverted_warm.cache_hit, "the v1 entry must serve warm");
    assert_eq!(reverted_warm.code, reverted.code);
}

/// A content upsert landing between compute and publish must DECLINE
/// the content-addressed publish: the fence's live-vs-captured compare
/// covers the CONTENT dimension, not just env/project identity.
///
/// Discrimination: pre-fix the publish rebuilt its live key FROM the
/// captured `whole_hash`, so a pure content movement compared equal on
/// every dimension and the publish landed (entry count 1); the fenced
/// publish re-reads the LIVE content hash and declines.
#[test]
fn content_mutation_between_compute_and_publish_declines_the_content_publish() {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    publish_graph(
        &workspace,
        vec![("@n/*".to_string(), vec!["./src/*".to_string()])],
    );
    let host = host_over(Arc::clone(&workspace));
    upsert(&host, FACT_FREE_V1);

    // Land the content upsert inside the compute→publish window, once.
    {
        let hook_host = Arc::clone(&host);
        let fired = std::sync::atomic::AtomicBool::new(false);
        *host.compile_publish_seam_hook.lock() = Some(Arc::new(move || {
            if !fired.swap(true, std::sync::atomic::Ordering::SeqCst) {
                upsert(&hook_host, FACT_FREE_V2);
            }
        }));
    }

    let profile = content_profile();
    let raced = compile(&host, &profile);
    *host.compile_publish_seam_hook.lock() = None;

    assert_eq!(
        raced.actual_mode,
        CompileCacheMode::Content,
        "fixture must classify to Content — the fence pin is otherwise vacuous",
    );
    assert!(!raced.cache_hit, "first compile must be cold");
    assert!(
        raced.code.contains("marker_v1"),
        "the declined publish must still serve the freshly compiled output",
    );
    // THE PIN: the content moved between compute and publish, so the
    // publish must DECLINE — the fence compares the live content hash,
    // never a rebuild from the captured one.
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        0,
        "a content mutation in the compute→publish window must decline \
         the content-addressed publish",
    );

    // Negative control: with the content now stable, the next compile
    // publishes normally and the one after warm-hits it.
    let republished = compile(&host, &profile);
    assert!(!republished.cache_hit, "no entry exists yet — cold");
    assert!(republished.code.contains("marker_v2"));
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        1,
        "a stable-content compile must publish exactly one content entry",
    );
    let warm = compile(&host, &profile);
    assert!(
        warm.cache_hit,
        "the stable entry must serve the next request warm"
    );
    assert_eq!(
        warm.code, republished.code,
        "warm hit must be byte-identical"
    );
}

/// Steady state (no mutation): the seam hook armed but mutating
/// NOTHING must not trip the fence — the captured and live identities
/// agree and the publish lands.
#[test]
fn stable_env_publish_lands_under_the_captured_identity() {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    publish_graph(
        &workspace,
        vec![("@n/*".to_string(), vec!["./src/*".to_string()])],
    );
    let host = host_over(Arc::clone(&workspace));
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(CANONICAL.to_string()),
            input_id: CANONICAL.to_string(),
            source: FACT_FREE.into(),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert vue");

    // A no-op hook: proves the fence compares identities rather than
    // declining whenever the seam fires.
    *host.compile_publish_seam_hook.lock() = Some(Arc::new(|| {}));
    let profile = content_profile();
    let cold = compile(&host, &profile);
    *host.compile_publish_seam_hook.lock() = None;

    assert_eq!(cold.actual_mode, CompileCacheMode::Content);
    assert_eq!(
        host.compile_output_pure_content_entry_count(),
        1,
        "an unmoved identity must publish exactly one content entry",
    );
    let warm = compile(&host, &profile);
    assert!(warm.cache_hit, "the published entry must serve warm");
}

/// The same coherent-snapshot discipline applies to `latest_diagnostics`,
/// which is NOT a keyed cache entry but observable state a reader trusts as
/// describing the CURRENT buffer.
///
/// A Vue template parse error makes `compile_entry` return `Err`, so this pair
/// drives the FAILURE write site — the one the LSP regression actually runs
/// through (`ensure_ide_compiled` answers `Err`, and the diagnostics the editor
/// shows were stored by that arm before it returned).
const BROKEN_V1: &str =
    "<script setup lang=\"ts\">const n = 1</script><template><div><span></div></template>";
const CLEAN_V2: &str =
    "<script setup lang=\"ts\">const n = 2</script><template><div>{{ n }}</div></template>";

fn try_compile(
    host: &VerterHost,
    profile: &CompileProfile,
) -> Result<crate::types::VirtualFileResponse, crate::types::HostError> {
    host.get_virtual_file(VirtualQuery {
        raw_id: None,
        canonical_id: Some(CANONICAL.to_string()),
        node_kind: Some(VirtualNodeKind::Main),
        compile_profile: profile.clone(),
    })
}

/// `None` = no slot at all (the state an upsert's clear leaves behind);
/// `Some(n)` = a compile wrote `n` diagnostics under this profile.
fn stored_diagnostics(host: &VerterHost, profile: &CompileProfile) -> Option<usize> {
    host.get_diagnostics(CANONICAL, profile)
        .map(|snapshot| snapshot.diagnostics.len())
}

/// A compile whose bytes are already superseded must NOT write its
/// diagnostics into the state the newer edit cleared.
///
/// `latest_diagnostics` has no key to reject a stale write: `get_diagnostics`
/// is a pure cached read, and its LSP consumers stamp what they read with the
/// document version THEY captured. So an in-flight v1 compile that lands its
/// parse errors after v2's upsert cleared the slot makes v1's errors
/// indistinguishable from v2's own — a publisher that captured v2, read the
/// slot, and passed its own document-identity fence publishes them as v2.
/// The user's file is clean and the editor shows errors that no longer exist
/// anywhere in the buffer, with nothing to clear them until the next edit.
///
/// Discrimination: pre-fence, both write sites inserted unconditionally, so
/// the raced compile left BROKEN_V1's ten template errors readable under the
/// live CLEAN_V2 content and the first assertion sees `Some(10)`. Post-fence
/// the write declines and the slot stays as the upsert left it. The recovery
/// legs pin that the fence compares identities rather than simply suppressing
/// writes — an always-declining "fix" fails them, and it is the same failure
/// mode (diagnostics that never arrive) this whole branch exists to repair.
#[test]
fn a_superseded_compile_never_writes_its_diagnostics_over_the_newer_revision() {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    publish_graph(
        &workspace,
        vec![("@n/*".to_string(), vec!["./src/*".to_string()])],
    );
    let host = host_over(Arc::clone(&workspace));
    upsert(&host, BROKEN_V1);

    // Land the CLEAN v2 upsert inside the snapshot→compile-input window, so
    // the compile below carries v1's bytes while the live buffer is v2.
    {
        let hook_host = Arc::clone(&host);
        let fired = std::sync::atomic::AtomicBool::new(false);
        *host.compile_input_seam_hook.lock() = Some(Arc::new(move || {
            if !fired.swap(true, std::sync::atomic::Ordering::SeqCst) {
                upsert(&hook_host, CLEAN_V2);
            }
        }));
    }
    let profile = content_profile();
    let raced = try_compile(&host, &profile);
    *host.compile_input_seam_hook.lock() = None;

    match raced {
        Err(crate::types::HostError::CompileError(failure)) => assert!(
            failure.diagnostics.has_errors,
            "precondition: the raced compile must have consumed the \
             request-start (v1) bytes and failed on them — otherwise there is \
             no superseded write to fence",
        ),
        other => panic!(
            "precondition: BROKEN_V1 must fail the compile under StrictError, \
             got {other:?}"
        ),
    }
    assert_eq!(
        stored_diagnostics(&host, &profile),
        None,
        "a compile whose source moved mid-flight must not write its \
         diagnostics: the live buffer is CLEAN_V2, so v1's template errors \
         would be read back and published stamped with v2's document version",
    );

    // Recovery leg 1: the buffer is stable at CLEAN_V2, so its own compile
    // writes — the fence compares identities, it does not suppress writes.
    // A `Some(0)` (not `None`) proves the SUCCESS write site landed too.
    let clean = try_compile(&host, &profile).expect("CLEAN_V2 compiles");
    assert!(
        clean.code.contains("const n = 2"),
        "the settled compile must serve the live revision",
    );
    assert_eq!(
        stored_diagnostics(&host, &profile),
        Some(0),
        "an unmoved successful compile must still write — a `None` here means \
         the fence declined a write for the live revision",
    );

    // Recovery leg 2: break it again with no race. The failure site's
    // diagnostics MUST land, or the fence has traded a stale-write bug for a
    // never-write bug — the exact regression this branch repairs.
    upsert(&host, BROKEN_V1);
    let broken = try_compile(&host, &profile);
    assert!(
        matches!(broken, Err(crate::types::HostError::CompileError(_))),
        "the settled compile must fail on the re-broken revision",
    );
    assert!(
        stored_diagnostics(&host, &profile).is_some_and(|count| count > 0),
        "an unmoved failing compile must still write its diagnostics — a fence \
         that declines unconditionally reproduces the empty-diagnostics \
         regression",
    );
}

/// The fenced writer's contract, exercised directly.
///
/// The end-to-end test above proves the fence fires on a real raced compile;
/// this pins the decision itself, independent of the compile pipeline, so a
/// refactor that moves the call sites cannot quietly drop it. Both the identity
/// check and the write are scoped to ONE compile-cache entry guard — that
/// placement is what makes it a fence and not a hint, because the clear it
/// races takes the very same entry.
#[test]
fn the_diagnostics_fence_declines_a_moved_revision_and_accepts_the_live_one() {
    let workspace = Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    publish_graph(
        &workspace,
        vec![("@n/*".to_string(), vec!["./src/*".to_string()])],
    );
    let host = host_over(Arc::clone(&workspace));

    let live_whole_hash = |host: &VerterHost| {
        host.scheduler()
            .try_get_source(CANONICAL)
            .and_then(|snap| {
                snap.downcast_data::<crate::host_executor::HostSourceData>()
                    .map(|data| data.parse.whole_hash)
            })
            .expect("the upserted canonical has a live source")
    };

    upsert(&host, BROKEN_V1);
    let v1 = live_whole_hash(&host);
    upsert(&host, CLEAN_V2);
    let v2 = live_whole_hash(&host);
    assert_ne!(v1, v2, "precondition: the two revisions must differ");

    let profile = content_profile();
    let profile_hash = crate::hash::compile_profile_hash(&profile);
    let some_diagnostics = crate::types::DiagnosticsSnapshot {
        diagnostics: vec![crate::types::HostDiagnostic {
            severity: crate::types::HostSeverity::Error,
            code: "XInvalidEndTag".to_string(),
            message: "Invalid end tag.".to_string(),
            arguments: Vec::new(),
            span: verter_span::Span::new(0, 1),
        }],
        has_errors: true,
    };

    assert!(
        !host.store_latest_diagnostics_if_source_unmoved(
            CANONICAL,
            profile_hash,
            v1,
            some_diagnostics.clone(),
        ),
        "a write carrying the SUPERSEDED revision's identity must be declined — \
         the live buffer is v2, and these diagnostics describe v1",
    );
    assert_eq!(
        stored_diagnostics(&host, &profile),
        None,
        "the declined write must leave the slot exactly as the upsert's clear \
         left it",
    );

    assert!(
        host.store_latest_diagnostics_if_source_unmoved(
            CANONICAL,
            profile_hash,
            v2,
            some_diagnostics.clone(),
        ),
        "a write carrying the LIVE revision's identity must land — a fence that \
         declines unconditionally strands the file with no diagnostics at all",
    );
    assert_eq!(
        stored_diagnostics(&host, &profile),
        Some(1),
        "the accepted write must be readable",
    );
}
