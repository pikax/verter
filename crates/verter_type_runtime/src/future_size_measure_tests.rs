//! Throwaway instrumentation: measure type-runtime async state-machine sizes.
//!
//! Run with:
//!   cargo test -p verter_type_runtime future_size_measure -- --nocapture --ignored
//!   cargo test -p verter_type_runtime future_size_measure --release -- --nocapture --ignored
//!
//! Not a gate. Numbers are printed and copied into docs/arch/future/* findings.

use std::mem::{size_of, size_of_val};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, oneshot, Mutex};

use crate::deadline::{with_deadline, with_deadline_at};
use crate::protocol::*;
use crate::traits::{ProviderFuture, ProviderPriority, TypeProvider};

fn report(label: &str, bytes: usize) {
    eprintln!(
        "[future-size] {label}: {bytes} B ({:.1} KiB)",
        bytes as f64 / 1024.0
    );
}

fn profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

/// Minimal TypeProvider that mirrors the production pattern: every method
/// returns `Box::pin(async move { … })` so outer sizes match the trait boundary.
struct MeasureMock;

impl TypeProvider for MeasureMock {
    fn provider_id(&self) -> &'static str {
        "measure"
    }

    fn open_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async move { Ok(()) })
    }

    fn update_file(&self, _path: &str, _content: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async move { Ok(()) })
    }

    fn close_file(&self, _path: &str) -> ProviderFuture<'_, ()> {
        Box::pin(async move { Ok(()) })
    }

    fn get_completions(
        &self,
        _path: &str,
        _offset: u32,
        _trigger: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        Box::pin(async move {
            Ok(CompletionResult {
                items: Vec::new(),
                is_incomplete: false,
            })
        })
    }

    fn get_hover(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        Box::pin(async move { Ok(None) })
    }

    fn get_diagnostics(&self, _path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn get_definition(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn get_type_definition(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn get_references(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn get_rename_locations(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn get_signature_help(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        Box::pin(async move { Ok(None) })
    }

    fn get_code_actions(
        &self,
        _path: &str,
        _start: u32,
        _end: u32,
        _diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn get_semantic_tokens(&self, _path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn get_document_highlights(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn get_inlay_hints(
        &self,
        _path: &str,
        _start: u32,
        _end: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        Box::pin(async move { Ok(Vec::new()) })
    }
}

/// Synthetic unboxed futures that mirror the transport's oneshot + timeout shape
/// without spawning a real tsgo child (the real `request_with_priority` body is
/// private on the transport).
async fn synthetic_oneshot_wait() -> Result<serde_json::Value, String> {
    let (tx, rx) = oneshot::channel::<serde_json::Value>();
    // Drop the sender so the wait completes immediately with Closed — we only
    // need the future's *layout* size, not a successful response.
    drop(tx);
    rx.await.map_err(|_| "closed".to_string())
}

async fn synthetic_timeout_oneshot() -> Result<serde_json::Value, String> {
    let (tx, rx) = oneshot::channel::<serde_json::Value>();
    drop(tx);
    match tokio::time::timeout(Duration::from_secs(10), rx).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(_)) => Err("closed".into()),
        Err(_) => Err("timeout".into()),
    }
}

#[tokio::test]
#[ignore = "throwaway instrumentation — run manually"]
async fn measure_type_runtime_future_sizes() {
    eprintln!("=== type_runtime futures profile={} ===", profile());

    // Trait-boundary outer sizes (production TypeProvider always boxes).
    let tp: Arc<dyn TypeProvider> = Arc::new(MeasureMock);
    {
        let fut = tp.get_definition("synthetic.tsx", 0);
        report("TypeProvider::get_definition (ProviderFuture)", size_of_val(&fut));
        drop(fut);
    }
    {
        let fut = tp.get_hover("synthetic.tsx", 0);
        report("TypeProvider::get_hover (ProviderFuture)", size_of_val(&fut));
        drop(fut);
    }
    {
        let fut = tp.get_completions("synthetic.tsx", 0, None);
        report(
            "TypeProvider::get_completions (ProviderFuture)",
            size_of_val(&fut),
        );
        drop(fut);
    }
    {
        let fut = tp.get_diagnostics("synthetic.tsx");
        report(
            "TypeProvider::get_diagnostics (ProviderFuture)",
            size_of_val(&fut),
        );
        drop(fut);
    }
    report("size_of ProviderFuture<()>", size_of::<ProviderFuture<'_, ()>>());

    // Deadline wrapper layers (public API; used by LSP audit harness).
    {
        let tiny = async { 7u8 };
        report("tiny async body", size_of_val(&tiny));
        let timed = tokio::time::timeout(Duration::from_secs(1), async { 7u8 });
        report("tokio::time::timeout(tiny)", size_of_val(&timed));
        let deadlined = with_deadline(Duration::from_secs(1), async { 7u8 });
        report("with_deadline(tiny)", size_of_val(&deadlined));
        let nested = with_deadline(
            Duration::from_secs(1),
            tokio::time::timeout(Duration::from_secs(1), async { 7u8 }),
        );
        report("with_deadline(timeout(tiny))", size_of_val(&nested));
        let at = with_deadline_at(tokio::time::Instant::now() + Duration::from_secs(1), async {
            7u8
        });
        report("with_deadline_at(tiny)", size_of_val(&at));
    }

    // Unboxed transport-shaped futures (oneshot wait / timeout+oneshot).
    {
        let fut = synthetic_oneshot_wait();
        report("synthetic oneshot wait (unboxed)", size_of_val(&fut));
        drop(fut);
    }
    {
        let fut = synthetic_timeout_oneshot();
        report(
            "synthetic timeout+oneshot (unboxed)",
            size_of_val(&fut),
        );
        drop(fut);
    }
    {
        // Mirror the production pattern: Box::pin around the transport shape.
        let fut: ProviderFuture<'_, serde_json::Value> =
            Box::pin(async move { synthetic_timeout_oneshot().await.map_err(|e| {
                crate::protocol::TypeProviderError::new(e)
            }) });
        report(
            "Box::pin(timeout+oneshot) ProviderFuture",
            size_of_val(&fut),
        );
        drop(fut);
    }

    // Production-shaped provider hop: captures path/uri + two Arcs + lock +
    // nested request (timeout+oneshot). This is the state that lives on the
    // HEAPinside Box::pin for TsgoTypeProvider::get_definition-class methods.
    {
        let path_owned = "synthetic.tsx".to_string();
        let uri = "file:///synthetic.tsx".to_string();
        let transport = Arc::new(());
        let contents = Arc::new(Mutex::new(std::collections::HashMap::<String, String>::new()));
        let fut = async move {
            let _line_char = {
                let guard = contents.lock().await;
                let _ = guard.get(&path_owned);
                (0u32, 0u32)
            };
            let _ = uri;
            let _ = transport;
            synthetic_timeout_oneshot().await
        };
        report(
            "unboxed get_definition-shaped hop (path+uri+Arcs+lock+timeout request)",
            size_of_val(&fut),
        );
        // Counterfactual: if N of these were held unboxed in a FuturesUnordered
        // (they are not — production boxes at ProviderFuture and JoinSet spawns).
        report(
            "counterfactual 8× unboxed definition-shaped hop",
            8 * size_of_val(&fut),
        );
        report(
            "counterfactual 64× unboxed definition-shaped hop",
            64 * size_of_val(&fut),
        );
        drop(fut);
    }
    {
        // Completion-detail per-item resolve task shape (clone item + Arc
        // transport + semaphore permit wait + request).
        let transport = Arc::new(());
        let semaphore = Arc::new(tokio::sync::Semaphore::new(8));
        let item_label = "foo".to_string();
        let fut = async move {
            let _permit = semaphore.acquire().await;
            let _ = (transport, item_label);
            synthetic_timeout_oneshot().await
        };
        report(
            "unboxed completion-detail resolve task shape",
            size_of_val(&fut),
        );
        report(
            "capacity×detail-task (8 concurrent unboxed, counterfactual)",
            8 * size_of_val(&fut),
        );
        drop(fut);
    }

    // Pending-map element sizes (maps hold oneshots, not futures).
    eprintln!("=== pending-map / channel element sizes ===");
    report(
        "size_of oneshot::Sender<serde_json::Value>",
        size_of::<oneshot::Sender<serde_json::Value>>(),
    );
    report(
        "size_of oneshot::Receiver<serde_json::Value>",
        size_of::<oneshot::Receiver<serde_json::Value>>(),
    );
    report(
        "size_of mpsc::Sender (lane element proxy)",
        size_of::<mpsc::Sender<Vec<u8>>>(),
    );
    // DEFAULT_LANE_CAPACITY in tsgo/ipc.rs is 1024; document capacity × oneshot
    // header if every slot held a pending entry (counterfactual full map).
    const DEFAULT_LANE_CAPACITY: usize = 1024;
    report("DEFAULT_LANE_CAPACITY", DEFAULT_LANE_CAPACITY);
    report(
        "capacity×oneshot::Sender header (if map full of pending)",
        DEFAULT_LANE_CAPACITY * size_of::<oneshot::Sender<serde_json::Value>>(),
    );

    // Completion-detail JoinSet concurrency constants (from tsgo/ipc.rs).
    const MAX_COMPLETION_DETAIL_ENRICH: usize = 50;
    const COMPLETION_DETAIL_RESOLVE_CONCURRENCY: usize = 8;
    report("MAX_COMPLETION_DETAIL_ENRICH", MAX_COMPLETION_DETAIL_ENRICH);
    report(
        "COMPLETION_DETAIL_RESOLVE_CONCURRENCY",
        COMPLETION_DETAIL_RESOLVE_CONCURRENCY,
    );
    // JoinSet stores JoinHandle (task already on heap). Inline handle size:
    report(
        "size_of JoinHandle<()>",
        size_of::<tokio::task::JoinHandle<()>>(),
    );
    report(
        "capacity×JoinHandle (8 concurrent detail tasks, inline)",
        COMPLETION_DETAIL_RESOLVE_CONCURRENCY * size_of::<tokio::task::JoinHandle<()>>(),
    );

    // ProviderPriority is Copy and tiny — not a future, but lives in request
    // routing state.
    report("size_of ProviderPriority", size_of::<ProviderPriority>());
    report(
        "size_of Arc<Mutex<HashMap>> header-ish (Pending map handle)",
        size_of::<Arc<Mutex<std::collections::HashMap<i64, oneshot::Sender<serde_json::Value>>>>>(),
    );
}
