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

#[tokio::test]
async fn failed_activation_is_memoized_and_shutdown_never_retries_or_spawns() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let provider = LazyManagedTypeProvider::new({
        let attempts = Arc::clone(&attempts);
        move || {
            let attempts = Arc::clone(&attempts);
            async move {
                attempts.fetch_add(1, Ordering::SeqCst);
                Err(verter_type_runtime::protocol::TypeProviderError::new(
                    "managed tsgo unavailable",
                ))
            }
        }
    });

    provider
        .open_file("/w/App.vue.tsx", "export {};")
        .await
        .unwrap();
    assert!(provider.get_hover("/w/App.vue.tsx", 0).await.is_err());
    assert!(provider.get_hover("/w/App.vue.tsx", 0).await.is_err());
    provider.shutdown().await.unwrap();
    assert_eq!(
        attempts.load(Ordering::SeqCst),
        1,
        "one failed fallback activation is memoized for the LSP session"
    );
}
