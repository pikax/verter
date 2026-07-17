//! Restart-replay and resolve-delegation coverage for the shared
//! [`ResilientProvider`](crate::resilient_provider::ResilientProvider).
//!
//! The restart mechanics live in `verter_type_runtime::resilient`, but the only
//! place a `TypeProvider` mock exists — and where `ResilientProvider` /
//! `ResilientBackend` / a `ProviderNotifier` impl are all reachable — is
//! `verter_lsp`. These tests therefore live here, exercising the wrapper through
//! its PUBLIC `TypeProvider` surface plus the recorded calls on a replacement
//! mock, never through private wrapper state.
//!
//! What they characterize:
//!   * a workspace-folder update issued WHILE the inner provider is down (mid
//!     restart) succeeds and survives to be replayed into the replacement;
//!   * after a crash, cached state is replayed into the replacement WITHOUT
//!     downgrading a `load_file`d file to an `open_file` (and vice-versa);
//!   * every cached path configuration is replayed;
//!   * `resolve_completion` is forwarded to the inner provider with its typed
//!     resolve handle intact.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::{Notify, Semaphore};

use crate::resilient_provider::{LspNotifier, ResilientBackend, ResilientProvider};
use crate::type_provider::mock::{MockCall, MockTypeProvider};
use crate::type_provider::protocol::{
    CompletionResolveData, CompletionResolveResult, ResolvedTextEdit,
};
use crate::type_provider::traits::TypeProvider;
use verter_type_runtime::protocol::TypeProviderError;

/// Backend that respawns a pre-built [`MockTypeProvider`].
///
/// `spawn` blocks on `spawn_gate` (a semaphore that starts with zero permits)
/// before handing back the replacement. The crash monitor clears the inner
/// provider BEFORE calling `spawn`, so holding the gate keeps the wrapper in its
/// "inner is down" (restarting) state deterministically — letting a test issue a
/// call while the provider is mid-restart without poking the wrapper's private
/// fields. Tests that don't need that window simply pre-release the gate.
struct TestBackend {
    replacement: MockTypeProvider,
    spawn_gate: Arc<Semaphore>,
}

impl ResilientBackend<MockTypeProvider> for TestBackend {
    fn log_name(&self) -> &'static str {
        "test-provider"
    }

    fn user_label(&self) -> &'static str {
        "test"
    }

    fn restarting_error(&self) -> &'static str {
        "test provider is restarting"
    }

    fn spawn<'a>(
        &'a self,
        _crash_notify: Arc<Notify>,
    ) -> Pin<Box<dyn Future<Output = Result<MockTypeProvider, TypeProviderError>> + Send + 'a>>
    {
        let provider = self.replacement.clone();
        let gate = Arc::clone(&self.spawn_gate);
        Box::pin(async move {
            // Block until the test releases a permit. A semaphore (not a
            // `Notify`) is used so a permit added before this point is not lost
            // — there is no register-then-release race.
            let permit = gate
                .acquire()
                .await
                .map_err(|_| TypeProviderError::new("test spawn gate closed"))?;
            permit.forget();
            Ok(provider)
        })
    }
}

/// Build a [`ResilientProvider`] over `initial`, respawning `replacement` on
/// crash. Returns the provider, the crash-notify handle, and the spawn gate.
///
/// The notifier is an [`LspNotifier`] over an unpopulated client cell: it only
/// logs / no-ops while the cell is empty, which is exactly the honest
/// observability seam these tests need.
fn make_resilient(
    initial: MockTypeProvider,
    replacement: MockTypeProvider,
) -> (
    ResilientProvider<MockTypeProvider, TestBackend>,
    Arc<Notify>,
    Arc<Semaphore>,
) {
    let crash_notify = Arc::new(Notify::new());
    let spawn_gate = Arc::new(Semaphore::new(0));
    let notifier = Arc::new(LspNotifier::new(Arc::new(tokio::sync::OnceCell::new())));
    let provider = ResilientProvider::new(
        initial,
        Arc::clone(&crash_notify),
        TestBackend {
            replacement,
            spawn_gate: Arc::clone(&spawn_gate),
        },
        notifier,
        3,
    );
    (provider, crash_notify, spawn_gate)
}

/// Drive the wrapper into its restarting (inner-down) state and confirm it.
///
/// `notify_waiters` only wakes tasks ALREADY parked on `notified()` (it stores
/// no permit, unlike `notify_one`), so a notify issued before the spawned crash
/// monitor has registered is lost. This re-notifies on every spin until a probe
/// query observes the down state — a `get_hover` returns the backend's
/// `restarting_error` ONLY while the inner provider is `None`. Because the
/// respawn is gated by the test's spawn permit, that down state is durable until
/// the test releases it, so this returns as soon as the monitor has cleared the
/// inner provider. Returns `true` once down, `false` if the deadline elapses.
async fn wait_for_restarting(
    provider: &ResilientProvider<MockTypeProvider, TestBackend>,
    crash_notify: &Notify,
) -> bool {
    // Generous deadline: the crash monitor sleeps ~1s (the first-attempt backoff)
    // AFTER clearing the inner provider but BEFORE reaching `spawn`, so the
    // down-state is observable for at least that long, and the spawn gate then
    // holds it down until the test releases a permit. 5s leaves ample slack on a
    // loaded machine without depending on the exact backoff value.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::time::Instant::now() < deadline {
        crash_notify.notify_waiters();
        if provider.get_hover("/probe.vue.tsx", 0).await.is_err() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    false
}

/// Wait until `replacement` has recorded at least `expected` calls, or the
/// deadline elapses. Returns the recorded calls regardless, so the caller's
/// assertions report the actual (possibly short) replay on failure.
async fn await_replayed_calls(replacement: &MockTypeProvider, expected: usize) -> Vec<MockCall> {
    // Generous deadline: when a test trips the crash WITHOUT first parking in
    // `wait_for_restarting`, the monitor's full ~1s pre-spawn backoff elapses
    // inside this poll window before any replay can land. 5s absorbs that sleep
    // plus scheduling jitter on a loaded machine.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while replacement.calls().len() < expected && std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    replacement.calls()
}

#[tokio::test]
async fn update_workspace_folders_is_cached_while_restarting() {
    let initial = MockTypeProvider::new();
    let replacement = MockTypeProvider::new();
    let replacement_clone = replacement.clone();
    // Gate the respawn so the wrapper stays in its restarting (inner-down) state
    // while we issue the workspace-folder update below.
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement);

    // Trip the crash monitor: it clears the inner provider, then blocks in
    // `spawn` on the gate, so the wrapper is now genuinely mid-restart. The
    // monitor is spawned inside `new()` and its first action is to await the
    // crash notify; `wait_for_restarting` re-notifies on each spin so a notify
    // issued before the monitor registered is not lost.
    let restarting = wait_for_restarting(&provider, &crash_notify).await;
    assert!(
        restarting,
        "inner provider should be down (restarting) after crash notify"
    );

    let added = vec![serde_json::json!({ "uri": "file:///workspace-a" })];
    let result = provider
        .update_workspace_folders(added.clone(), vec![])
        .await;

    assert!(
        result.is_ok(),
        "workspace folder cache update should succeed while restarting, got {result:?}"
    );

    // Release the respawn. The folder cached above must replay into the
    // replacement — proving it was retained across the restart rather than lost
    // while the inner provider was down.
    spawn_gate.add_permits(1);

    let calls = await_replayed_calls(&replacement_clone, 1).await;
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::UpdateWorkspaceFolders { added, .. }
                if added.iter().any(|folder|
                    folder.get("uri").and_then(|value| value.as_str()) == Some("file:///workspace-a"))
        )),
        "workspace folders set while restarting should be cached and replayed, calls={calls:?}"
    );
}

#[tokio::test]
async fn restart_replays_cached_state_without_downgrading_loaded_files() {
    let initial = MockTypeProvider::new();
    let replacement = MockTypeProvider::new();
    let replacement_clone = replacement.clone();
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement);

    provider
        .load_file("/project/src/loaded.vue.tsx", "const loaded = true;")
        .await
        .unwrap();
    provider
        .update_file("/project/src/loaded.vue.tsx", "const loaded = 2;")
        .await
        .unwrap();
    provider
        .open_file("/project/src/open.vue.tsx", "const open = true;")
        .await
        .unwrap();
    provider
        .configure_paths("/project/src", serde_json::json!({ "@/*": ["./*"] }))
        .await
        .unwrap();
    provider
        .update_workspace_folders(
            vec![serde_json::json!({ "uri": "file:///project" })],
            vec![],
        )
        .await
        .unwrap();

    assert!(
        wait_for_restarting(&provider, &crash_notify).await,
        "inner provider should be down (restarting) after crash notify"
    );
    // Allow the respawn to proceed once the monitor reaches `spawn`.
    spawn_gate.add_permits(1);

    let calls = await_replayed_calls(&replacement_clone, 4).await;
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::LoadFile { path, .. } if path == "/project/src/loaded.vue.tsx"
        )),
        "loaded files should replay via load_file, calls={calls:?}"
    );
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } if path == "/project/src/loaded.vue.tsx"
        )),
        "loaded files must not be replayed as open files, calls={calls:?}"
    );
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } if path == "/project/src/open.vue.tsx"
        )),
        "open files should replay via open_file, calls={calls:?}"
    );
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::ConfigurePaths { base_url, .. } if base_url == "/project/src"
        )),
        "path configuration should be replayed after restart, calls={calls:?}"
    );
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::UpdateWorkspaceFolders { added, .. }
                if added.iter().any(|folder| folder.get("uri").and_then(|value| value.as_str()) == Some("file:///project"))
        )),
        "workspace folders should be replayed after restart, calls={calls:?}"
    );
}

#[tokio::test]
async fn restart_replays_all_cached_path_configs() {
    let initial = MockTypeProvider::new();
    let replacement = MockTypeProvider::new();
    let replacement_clone = replacement.clone();
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement);

    provider
        .configure_paths("/project/pkg-a", serde_json::json!({ "@a/*": ["./src/*"] }))
        .await
        .unwrap();
    provider
        .configure_paths("/project/pkg-b", serde_json::json!({ "@b/*": ["./lib/*"] }))
        .await
        .unwrap();

    assert!(
        wait_for_restarting(&provider, &crash_notify).await,
        "inner provider should be down (restarting) after crash notify"
    );
    spawn_gate.add_permits(1);

    let calls = await_replayed_calls(&replacement_clone, 2).await;
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::ConfigurePaths { base_url, paths }
                if base_url == "/project/pkg-a"
                    && *paths == serde_json::json!({ "@a/*": ["./src/*"] })
        )),
        "restart should replay pkg-a path configuration, calls={calls:?}"
    );
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::ConfigurePaths { base_url, paths }
                if base_url == "/project/pkg-b"
                    && *paths == serde_json::json!({ "@b/*": ["./lib/*"] })
        )),
        "restart should replay pkg-b path configuration, calls={calls:?}"
    );
}

#[tokio::test]
async fn register_carrier_member_forwards_and_replays_after_restart() {
    let initial = MockTypeProvider::new();
    let replacement = MockTypeProvider::new();
    let initial_clone = initial.clone();
    let replacement_clone = replacement.clone();
    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement);

    // (i) forward + (ii) cache: a carrier companion registered on the wrapper must
    // reach the LIVE inner provider (the wrapper must not swallow it in a trait
    // default no-op — that no-op was the production bug: carriers never registered
    // with tsserver because the publish path called the wrapper, not the inner).
    provider
        .register_carrier_member(
            "/project/src/App.vue",
            "/project/src/App.vue.tsx",
            "export default {} as any;\n",
            "/project/tsconfig.json",
        )
        .await
        .unwrap();

    assert!(
        initial_clone.calls().iter().any(|call| matches!(
            call,
            MockCall::RegisterCarrierMember {
                source_path,
                companion_path,
                project_file_name,
                ..
            } if companion_path == "/project/src/App.vue.tsx"
                && source_path == "/project/src/App.vue"
                && project_file_name == "/project/tsconfig.json"
        )),
        "register_carrier_member must forward to the live inner provider, calls={:?}",
        initial_clone.calls()
    );

    // Drive the wrapper into its restarting (inner-down) state, then release the
    // respawn.
    assert!(
        wait_for_restarting(&provider, &crash_notify).await,
        "inner provider should be down (restarting) after crash notify"
    );
    spawn_gate.add_permits(1);

    // (iii) replay: the cached carrier registration is re-issued to the FRESH inner
    // after restart — carrying the SAME contentless registration payload — so the
    // carrier re-enters its owning configured project on the replacement provider
    // before user-facing requests can hit it.
    let calls = await_replayed_calls(&replacement_clone, 1).await;
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::RegisterCarrierMember {
                source_path,
                companion_path,
                content,
                project_file_name,
            } if companion_path == "/project/src/App.vue.tsx"
                && source_path == "/project/src/App.vue"
                && content == "export default {} as any;\n"
                && project_file_name == "/project/tsconfig.json"
        )),
        "a cached carrier registration must replay into the replacement after restart, calls={calls:?}"
    );
}

/// Production path: a `register_carrier_member` racing the respawn's carrier
/// snapshot → replay → inner-swap MUST reach the fresh inner, never be lost.
///
/// The window: a registration that caches AFTER the respawn snapshots its carrier
/// set but BEFORE the inner is swapped in would (pre-fix) see `inner = None`, return
/// success-as-cached, and be in NEITHER the replay set NOR the live inner. The fix
/// serializes registration against the snapshot→replay→swap via a gate.
///
/// This drives the PRODUCTION `ResilientProvider` wrapper: it pauses the real
/// respawn INSIDE its carrier replay (by blocking the replacement mock's
/// `register_carrier_member` on the replayed carrier A — the gate-held window) and
/// then issues a registration for a DIFFERENT carrier B through the wrapper's
/// public surface. RED before the fix: B caches post-snapshot, sees `inner = None`,
/// and never reaches the replacement; GREEN: B blocks on the gate until the swap,
/// then forwards to the replacement.
#[tokio::test(flavor = "multi_thread")]
async fn registration_racing_respawn_replay_reaches_fresh_inner() {
    let initial = MockTypeProvider::new();
    let replacement = MockTypeProvider::new();
    let replacement_clone = replacement.clone();

    let carrier_a = "/project/src/A.vue.tsx";
    let carrier_b = "/project/src/B.vue.tsx";

    // Pause the respawn's REPLAY of carrier A inside the gate-held window: the
    // replacement's `register_carrier_member(A)` records then blocks until released.
    let release_a = replacement.block_register_carrier_member(carrier_a);

    let (provider, crash_notify, spawn_gate) = make_resilient(initial, replacement);
    let provider = Arc::new(provider);

    // Register carrier A on the wrapper BEFORE the crash so it is in the respawn's
    // replay snapshot.
    provider
        .register_carrier_member(
            "/project/src/A.vue",
            carrier_a,
            "export default {} as any; // A\n",
            "/project/tsconfig.json",
        )
        .await
        .unwrap();

    // Trip the crash; the monitor clears the inner and parks on the spawn gate.
    assert!(
        wait_for_restarting(&provider, &crash_notify).await,
        "inner provider should be down (restarting) after crash notify"
    );
    // Release the respawn: it spawns the replacement, then replays carrier A —
    // which BLOCKS in the replacement mock, holding the registration gate across
    // the snapshot→swap window.
    spawn_gate.add_permits(1);

    // Wait until A's replay has been recorded — the respawn is now PAUSED mid-replay
    // (carrier snapshot taken, inner not yet swapped, registration gate held).
    let replayed = await_replayed_calls(&replacement_clone, 1).await;
    assert!(
        replayed.iter().any(|call| matches!(
            call,
            MockCall::RegisterCarrierMember { companion_path, .. } if companion_path == carrier_a
        )),
        "the respawn must be paused replaying carrier A (the gate-held window), calls={replayed:?}"
    );

    // Issue carrier B THROUGH the wrapper while the respawn is paused in the window.
    let provider_for_b = Arc::clone(&provider);
    let task_b = tokio::spawn(async move {
        provider_for_b
            .register_carrier_member(
                "/project/src/B.vue",
                carrier_b,
                "export default {} as any; // B\n",
                "/project/tsconfig.json",
            )
            .await
    });

    // No settle needed: the single-writer actor serialises B's command behind the
    // respawn through its command channel, so B is enqueued and processed after the
    // swap regardless of timing — the assertion below blocks on the recorded calls
    // (`await_replayed_calls`), not on a wall-clock delay.

    // Release A → the respawn finishes the replay, swaps in the fresh inner, and
    // drops the gate. GREEN: B now forwards to the replacement.
    release_a.notify_one();
    task_b
        .await
        .expect("B task joins")
        .expect("B registration ok");

    // The fresh inner (replacement) MUST have received carrier B — either by replay
    // (had it landed before the snapshot) or by the gate-serialized forward. RED:
    // B was lost in the snapshot→swap window and never reached the replacement.
    let calls = await_replayed_calls(&replacement_clone, 2).await;
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::RegisterCarrierMember { companion_path, .. } if companion_path == carrier_b
        )),
        "a registration racing the respawn replay MUST reach the fresh inner (not be \
         lost in the snapshot→swap window), calls={calls:?}"
    );
}

#[tokio::test]
async fn resolve_completion_delegates_to_the_inner_provider() {
    let initial = MockTypeProvider::new();
    let replacement = MockTypeProvider::new();

    // Typed resolve handle (not a bare JSON value): the mock keys its configured
    // response on the exact `CompletionResolveData` it receives.
    let key = CompletionResolveData::Lsp {
        label: "Foo".into(),
        data: serde_json::json!({ "kind": "import" }),
    };
    initial.set_resolve_completion(
        "/project/src/App.vue.tsx",
        key.clone(),
        Some(CompletionResolveResult {
            additional_text_edits: vec![ResolvedTextEdit {
                start: 0,
                end: 0,
                new_text: "import Foo from './Foo';".to_string(),
            }],
            detail: None,
            documentation: None,
            ..Default::default()
        }),
    );
    let (provider, _crash_notify, _spawn_gate) = make_resilient(initial.clone(), replacement);

    let result = provider
        .resolve_completion("/project/src/App.vue.tsx", key.clone())
        .await
        .unwrap();

    assert_eq!(
        result.map(|resolved| resolved.additional_text_edits.len()),
        Some(1),
        "resolve result should be forwarded back from the inner provider"
    );
    assert!(
        initial.calls().iter().any(|call| matches!(
            call,
            MockCall::ResolveCompletion { path, .. } if path == "/project/src/App.vue.tsx"
        )),
        "resolve_completion should be forwarded to the inner provider"
    );
}
