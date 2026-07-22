//! Throwaway instrumentation: measure tsgo_api / relay async future sizes.
//!
//! Run with:
//!   cargo test -p verter_tsgo_api future_size_measure -- --nocapture --ignored
//!   cargo test -p verter_tsgo_api future_size_measure --release -- --nocapture --ignored
//!
//! Not a gate. Numbers are printed and copied into docs/arch/future/* findings.

use std::mem::{size_of, size_of_val};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::{mpsc, oneshot};

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

/// Shape of a JsonRpcConnection::request future (oneshot + pending-guard +
/// channel send) without a live transport.
async fn synthetic_jsonrpc_request_shape() -> Result<serde_json::Value, String> {
    let pending: Arc<Mutex<std::collections::HashMap<i64, oneshot::Sender<serde_json::Value>>>> =
        Arc::new(Mutex::new(std::collections::HashMap::new()));
    let id = 1i64;
    let (tx, rx) = oneshot::channel();
    pending.lock().insert(id, tx);
    // Drop the sender via remove so the wait ends; layout is what matters.
    let _ = pending.lock().remove(&id);
    rx.await.map_err(|_| "closed".to_string())
}

async fn synthetic_actor_request_shape() -> Result<Vec<u8>, String> {
    let (reply_tx, reply_rx) = oneshot::channel::<Result<Vec<u8>, String>>();
    let (lane_tx, mut lane_rx) = mpsc::channel::<oneshot::Sender<Result<Vec<u8>, String>>>(4);
    // Enqueue then immediately close so the await path is constructible.
    let _ = lane_tx.send(reply_tx).await;
    drop(lane_tx);
    if let Some(tx) = lane_rx.recv().await {
        let _ = tx.send(Ok(vec![1, 2, 3]));
    }
    reply_rx.await.map_err(|_| "closed".to_string())?
}

#[tokio::test]
#[ignore = "throwaway instrumentation — run manually"]
async fn measure_tsgo_api_future_sizes() {
    eprintln!("=== tsgo_api futures profile={} ===", profile());

    {
        let fut = synthetic_jsonrpc_request_shape();
        report(
            "synthetic JsonRpcConnection::request shape (unboxed)",
            size_of_val(&fut),
        );
        drop(fut);
    }
    {
        let fut = synthetic_actor_request_shape();
        report(
            "synthetic actor ClientHandle::request shape (unboxed)",
            size_of_val(&fut),
        );
        drop(fut);
    }
    {
        let timed =
            tokio::time::timeout(Duration::from_secs(10), synthetic_jsonrpc_request_shape());
        report("timeout(jsonrpc request shape)", size_of_val(&timed));
        drop(timed);
    }
    {
        // Select with cancel — mirrors ClientHandle::request when a cancel
        // token is provided.
        let (reply_tx, reply_rx) = oneshot::channel::<Result<Vec<u8>, String>>();
        drop(reply_tx);
        let cancel = async {
            tokio::time::sleep(Duration::from_millis(2)).await;
        };
        let fut = async {
            tokio::select! {
                biased;
                res = reply_rx => res.map_err(|_| "closed".to_string())?,
                () = cancel => Err("cancelled".to_string()),
            }
        };
        report("select(reply_rx, cancel) request shape", size_of_val(&fut));
        drop(fut);
    }

    eprintln!("=== pending-map / actor queue element sizes ===");
    report(
        "size_of oneshot::Sender<serde_json::Value>",
        size_of::<oneshot::Sender<serde_json::Value>>(),
    );
    report(
        "size_of oneshot::Receiver<serde_json::Value>",
        size_of::<oneshot::Receiver<serde_json::Value>>(),
    );
    report(
        "size_of oneshot::Sender<Result<Vec<u8>, ()>>",
        size_of::<oneshot::Sender<Result<Vec<u8>, ()>>>(),
    );
    // Actor queues hold ActorRequest (method String + payload Vec + oneshot),
    // not the caller's future. Approximate header:
    report("size_of String header", size_of::<String>());
    report("size_of Vec<u8> header", size_of::<Vec<u8>>());
    report(
        "size_of Option<Duration> (deadline field)",
        size_of::<Option<Duration>>(),
    );

    // Box pin used by SpawnOwnTsgoLsp connection source.
    type BoxFut = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;
    report("size_of BoxFuture slot", size_of::<BoxFut>());
}
