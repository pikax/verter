//! The server-scoped interactive-handler activity: background admission is
//! gated by the ONE activity the server owns — a handler in flight on one
//! server holds back that server's background work and nobody else's — and
//! the server instance hands that same activity to its guard substrate.

use std::future::Future;
use std::sync::Arc;
use std::task::Poll;

use verter_lsp::server::{HandlerActivity, HandlerGuard, VerterLanguageServer};
use verter_lsp::{LspConfig, ProjectSyncMode, TypeProviderKind};
use verter_session::{HostConfig, VerterHost};

fn build_test_server() -> VerterLanguageServer {
    VerterLanguageServer::new(
        verter_lsp::outbound::Outbound::default(),
        LspConfig {
            host: Arc::new(VerterHost::new_standalone(HostConfig::default())),
            type_provider: None,
            type_provider_topology: verter_lsp::TypeProviderTopology::None,
            project_sync_mode: ProjectSyncMode::FullProject,
            type_provider_kind: TypeProviderKind::Tsserver,
            mcp_port: None,
            type_provider_reason: None,
            type_provider_advisory: None,
            suppress_imported_carrier_prewarm: false,
        },
    )
}

/// Background admission is scoped to its server: a handler in flight on
/// one activity holds back that activity's background work and nobody
/// else's, and releasing it wakes its own waiter.
#[tokio::test(start_paused = true)]
async fn handler_activity_gates_only_its_own_servers_background_work() {
    let busy = HandlerActivity::default();
    let other = HandlerActivity::default();
    let quiet = std::time::Duration::from_millis(30);
    let start = tokio::time::Instant::now();
    let handler = HandlerGuard::new(&busy, "hover");

    let mut busy_waiter = Box::pin(busy.wait_idle());
    let mut ctx = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(
        matches!(busy_waiter.as_mut().poll(&mut ctx), Poll::Pending),
        "a server's own in-flight handler holds back its background work"
    );
    other.wait_idle().await;
    assert!(
        other
            .wait_quiet(quiet, std::time::Duration::from_secs(1))
            .await,
        "another server's handler must not consume this server's quiet window"
    );
    assert_eq!(tokio::time::Instant::now(), start + quiet);

    drop(handler);
    busy_waiter.await;
    assert_eq!(busy.active(), 0);
}

/// The server's ONE handler activity is the guard substrate: a guard taken
/// on the activity the server exposes holds back admission through that
/// same activity, and releases it on drop. The background scanner and
/// heartbeat receive their activity only from the server instance itself
/// (`BackgroundInitArgs::from_server` / `spawn_heartbeat`), so this is the
/// single activity all three share.
#[tokio::test(start_paused = true)]
async fn the_servers_handlers_and_background_work_share_one_activity() {
    let server = build_test_server();

    let activity = Arc::clone(server.handler_activity());
    let handler = HandlerGuard::new(&activity, "hover");
    let mut waiter = Box::pin(activity.wait_idle());
    let mut ctx = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(
        matches!(waiter.as_mut().poll(&mut ctx), Poll::Pending),
        "a guard on the server's activity must hold back that server's \
         background admission"
    );
    drop(handler);
    waiter.await;
    assert_eq!(activity.active(), 0);
}
