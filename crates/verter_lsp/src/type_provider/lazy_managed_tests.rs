use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use verter_type_runtime::traits::TypeProvider;

use super::lazy_managed::LazyManagedTypeProvider;
use super::mock::{MockCall, MockTypeProvider};

#[tokio::test]
async fn lifecycle_is_cached_without_activation_and_replayed_once_on_first_query() {
    let spawn_count = Arc::new(AtomicUsize::new(0));
    let managed = Arc::new(MockTypeProvider::new());
    let provider = LazyManagedTypeProvider::new({
        let spawn_count = Arc::clone(&spawn_count);
        let managed = Arc::clone(&managed);
        move || {
            let spawn_count = Arc::clone(&spawn_count);
            let managed = Arc::clone(&managed);
            async move {
                spawn_count.fetch_add(1, Ordering::SeqCst);
                Ok(managed as Arc<dyn TypeProvider>)
            }
        }
    });

    provider
        .open_file("/w/App.vue.tsx", "const before = 1")
        .await
        .unwrap();
    provider
        .update_file("/w/App.vue.tsx", "const current = 2")
        .await
        .unwrap();
    provider
        .load_file("/w/Closed.vue.tsx", "export const gone = true")
        .await
        .unwrap();
    provider.close_file("/w/Closed.vue.tsx").await.unwrap();
    provider
        .configure_paths("/w", serde_json::json!({ "@/*": ["src/*"] }))
        .await
        .unwrap();

    assert_eq!(
        spawn_count.load(Ordering::SeqCst),
        0,
        "file lifecycle must not start the managed fallback"
    );
    assert!(
        managed.calls().is_empty(),
        "nothing reaches managed tsgo before an observed fallback demand"
    );

    provider.get_hover("/w/App.vue.tsx", 8).await.unwrap();
    provider.get_hover("/w/App.vue.tsx", 8).await.unwrap();

    assert_eq!(
        spawn_count.load(Ordering::SeqCst),
        1,
        "repeated fallback demands reuse exactly one managed provider"
    );
    let calls = managed.calls();
    assert!(
        matches!(calls.first(), Some(MockCall::ConfigurePaths { .. })),
        "configuration must replay before file state: {calls:?}"
    );
    assert!(
        calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, content }
                if path == "/w/App.vue.tsx" && content == "const current = 2"
        )),
        "activation must replay the latest desired open state before the query: {calls:?}"
    );
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            MockCall::OpenFile { path, .. } | MockCall::LoadFile { path, .. }
                if path == "/w/Closed.vue.tsx"
        )),
        "a file closed before activation must not be replayed: {calls:?}"
    );
    assert_eq!(
        calls
            .iter()
            .filter(|call| matches!(call, MockCall::GetHover { .. }))
            .count(),
        2
    );
}

#[tokio::test]
async fn concurrent_first_queries_singleflight_the_managed_factory() {
    let spawn_count = Arc::new(AtomicUsize::new(0));
    let managed = Arc::new(MockTypeProvider::new());
    let provider = Arc::new(LazyManagedTypeProvider::new({
        let spawn_count = Arc::clone(&spawn_count);
        let managed = Arc::clone(&managed);
        move || {
            let spawn_count = Arc::clone(&spawn_count);
            let managed = Arc::clone(&managed);
            async move {
                spawn_count.fetch_add(1, Ordering::SeqCst);
                tokio::task::yield_now().await;
                Ok(managed as Arc<dyn TypeProvider>)
            }
        }
    }));

    let (left, right) = tokio::join!(
        provider.get_hover("/w/App.vue.tsx", 1),
        provider.get_hover("/w/App.vue.tsx", 2)
    );
    left.unwrap();
    right.unwrap();
    assert_eq!(spawn_count.load(Ordering::SeqCst), 1);
}

/// D2: a transient activation failure is NOT latched for the session. Within the
/// cooldown the cached error is returned WITHOUT re-running the factory (storm
/// protection); after the cooldown the factory is retried and the provider
/// recovers, so the next fallback query answers.
#[tokio::test]
async fn failed_activation_retries_after_cooldown_and_recovers() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let fail = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let managed = Arc::new(MockTypeProvider::new());
    let provider = LazyManagedTypeProvider::new({
        let attempts = Arc::clone(&attempts);
        let fail = Arc::clone(&fail);
        let managed = Arc::clone(&managed);
        move || {
            let attempts = Arc::clone(&attempts);
            let fail = Arc::clone(&fail);
            let managed = Arc::clone(&managed);
            async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                if fail.load(Ordering::SeqCst) {
                    Err(verter_type_runtime::protocol::TypeProviderError::new(
                        "managed tsgo unavailable",
                    ))
                } else {
                    Ok(managed as Arc<dyn TypeProvider>)
                }
            }
        }
    });

    provider
        .open_file("/w/App.vue.tsx", "export {};")
        .await
        .unwrap();
    // First activation fails (attempt 1).
    assert!(provider.get_hover("/w/App.vue.tsx", 0).await.is_err());
    // Immediate retry: cached error, NO new factory run (storm protection).
    assert!(provider.get_hover("/w/App.vue.tsx", 0).await.is_err());
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "a failure within the cooldown is returned without re-running the factory"
    );

    // After the cooldown the factory is retried; now it succeeds.
    fail.store(false, Ordering::SeqCst);
    tokio::time::sleep(std::time::Duration::from_millis(
        super::lazy_managed::ACTIVATION_RETRY_COOLDOWN.as_millis() as u64 + 80,
    ))
    .await;
    provider
        .get_hover("/w/App.vue.tsx", 0)
        .await
        .expect("a transient activation failure must recover after the cooldown");
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        2,
        "the factory is retried exactly once after the cooldown"
    );
    provider.shutdown().await.unwrap();
}
