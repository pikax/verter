//! Server-side protocol diagnostic timeline (WSP1).
//!
//! Records per-request stage timestamps that separate compiler/provider
//! work, serialization, outbound queues and transport from client-side
//! stages. Source text is never stored. Disabled by default.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use parking_lot::Mutex;
use serde::Serialize;

/// Ordered server-side stages. Client decode/apply/paint are WSP1L.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolStage {
    RequestReceived,
    Admitted,
    ProviderWork,
    Serialize,
    OutboundEnqueued,
    OutboundWritten,
    Complete,
}

impl ProtocolStage {
    /// Pipeline order used to pick the first blocked server stage.
    pub const ORDER: &'static [ProtocolStage] = &[
        ProtocolStage::RequestReceived,
        ProtocolStage::Admitted,
        ProtocolStage::ProviderWork,
        ProtocolStage::Serialize,
        ProtocolStage::OutboundEnqueued,
        ProtocolStage::OutboundWritten,
        ProtocolStage::Complete,
    ];

    fn rank(self) -> usize {
        Self::ORDER
            .iter()
            .position(|stage| *stage == self)
            .unwrap_or(usize::MAX)
    }
}

/// One stage observation. `source` is never recorded.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StageStamp {
    pub stage: ProtocolStage,
    /// Milliseconds from session start (monotonic).
    pub at_ms: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_length: Option<u64>,
}

/// Terminal state of one request's timeline. Failed and cancelled requests
/// never carry a `complete` stamp: partial, pending, failed and cancelled stay
/// distinct from complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TraceStatus {
    #[default]
    Pending,
    Complete,
    Failed,
    Cancelled,
}

/// One request's timeline, keyed by request epoch and optional source epoch.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionTrace {
    pub request_epoch: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_epoch: Option<u64>,
    pub method: String,
    pub status: TraceStatus,
    pub stamps: Vec<StageStamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_blocked_stage: Option<ProtocolStage>,
}

/// Snapshot returned on `$/verter/getStatistics` when tracing is enabled.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionTraceSnapshot {
    pub enabled: bool,
    pub traces: Vec<InteractionTrace>,
    /// Metrics this node does not instrument; never guessed as zero.
    pub unavailable_metrics: &'static [&'static str],
}

/// Opt-in collector. Recording is a no-op when disabled.
pub struct InteractionTraceLog {
    enabled: AtomicBool,
    next_epoch: AtomicU64,
    session_start: Instant,
    session_start_unix_ms: u64,
    traces: Mutex<Vec<InteractionTrace>>,
    max_traces: usize,
    /// Optional harness dump (`VERTER_LSP_INTERACTION_TRACE_DUMP`): one JSONL
    /// line per terminal trace state, plus the session anchor written first.
    dump_path: Option<std::path::PathBuf>,
}

impl InteractionTraceLog {
    pub fn new(max_traces: usize) -> Self {
        let dump_path =
            std::env::var_os("VERTER_LSP_INTERACTION_TRACE_DUMP").map(std::path::PathBuf::from);
        let log = Self {
            enabled: AtomicBool::new(std::env::var_os("VERTER_LSP_INTERACTION_TRACE").is_some()),
            next_epoch: AtomicU64::new(1),
            session_start: Instant::now(),
            session_start_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            traces: Mutex::new(Vec::new()),
            max_traces,
            dump_path,
        };
        if let Some(path) = &log.dump_path {
            // The anchor joins this session's relative-ms stamps to the Unix
            // clock the UI stamps use (WSP1L.2 correlation).
            let anchor = serde_json::json!({
                "anchor": "session",
                "sessionStartUnixMs": log.session_start_unix_ms,
            });
            if let Err(err) = std::fs::write(path, format!("{anchor}\n")) {
                tracing::warn!("interaction-trace dump anchor failed: {err}");
            }
        }
        log
    }

    /// The Unix-ms reading of this session's t=0 (dump correlation anchor).
    pub fn session_start_unix_ms(&self) -> u64 {
        self.session_start_unix_ms
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    pub fn now_ms(&self) -> f64 {
        self.session_start.elapsed().as_secs_f64() * 1000.0
    }

    /// Open a request span. `source_epoch` is evaluated only when tracing is
    /// enabled; returns a no-op span when disabled.
    pub fn begin(&self, method: &str, source_epoch: impl FnOnce() -> Option<u64>) -> TraceSpan<'_> {
        if !self.is_enabled() {
            return TraceSpan {
                log: self,
                epoch: 0,
                live: false,
            };
        }
        let source_epoch = source_epoch();
        let epoch = self.next_epoch.fetch_add(1, Ordering::Relaxed);
        let trace = InteractionTrace {
            request_epoch: epoch,
            source_epoch,
            method: method.to_string(),
            status: TraceStatus::Pending,
            stamps: vec![StageStamp {
                stage: ProtocolStage::RequestReceived,
                at_ms: self.now_ms(),
                byte_length: None,
            }],
            first_blocked_stage: None,
        };
        let mut traces = self.traces.lock();
        if traces.len() >= self.max_traces {
            traces.remove(0);
        }
        traces.push(trace);
        TraceSpan {
            log: self,
            epoch,
            live: true,
        }
    }

    pub fn snapshot(&self) -> InteractionTraceSnapshot {
        let traces = self.traces.lock().clone();
        InteractionTraceSnapshot {
            enabled: self.is_enabled(),
            traces,
            unavailable_metrics: &[
                "client_input_to_paint",
                "decode_apply_time",
                "process_tree_rss",
                "language_service_incremental_rss",
            ],
        }
    }

    fn with_trace(&self, epoch: u64, f: impl FnOnce(&mut InteractionTrace)) {
        if !self.is_enabled() {
            return;
        }
        let mut traces = self.traces.lock();
        if let Some(trace) = traces.iter_mut().rev().find(|t| t.request_epoch == epoch) {
            f(trace);
        }
    }
}

/// RAII span for one request. The terminal state is recorded explicitly:
/// [`TraceSpan::finish_ok`] on success, [`TraceSpan::fail`] on error. A span
/// dropped while still live was cancelled or abandoned mid-flight and is
/// recorded as `cancelled` — never as a completion.
pub struct TraceSpan<'a> {
    log: &'a InteractionTraceLog,
    epoch: u64,
    live: bool,
}

impl TraceSpan<'_> {
    pub fn request_epoch(&self) -> Option<u64> {
        self.live.then_some(self.epoch)
    }

    pub fn mark(&self, stage: ProtocolStage) {
        self.mark_with_bytes(stage, None);
    }

    pub fn mark_with_bytes(&self, stage: ProtocolStage, byte_length: Option<u64>) {
        if !self.live {
            return;
        }
        let at_ms = self.log.now_ms();
        self.log.with_trace(self.epoch, |trace| {
            trace.stamps.push(StageStamp {
                stage,
                at_ms,
                byte_length,
            });
        });
    }

    /// Record a successful completion: appends the `complete` stamp and
    /// computes the first blocked stage over the observed server stamps.
    pub fn finish_ok(self) {
        self.record_terminal(TraceStatus::Complete, true);
    }

    /// Record a failed request. No `complete` stamp and no blocking analysis:
    /// a failure must not masquerade as a completed timeline.
    pub fn fail(self) {
        self.record_terminal(TraceStatus::Failed, false);
    }

    fn record_terminal(&self, status: TraceStatus, stamp_complete: bool) {
        if !self.live {
            return;
        }
        let at_ms = self.log.now_ms();
        self.log.with_trace(self.epoch, |trace| {
            if trace.status != TraceStatus::Pending {
                return;
            }
            if stamp_complete
                && !trace
                    .stamps
                    .iter()
                    .any(|stamp| stamp.stage == ProtocolStage::Complete)
            {
                trace.stamps.push(StageStamp {
                    stage: ProtocolStage::Complete,
                    at_ms,
                    byte_length: None,
                });
            }
            trace.status = status;
            trace.first_blocked_stage = match status {
                TraceStatus::Complete => first_blocked_server_stage(&trace.stamps),
                _ => None,
            };
            if let Some(path) = &self.log.dump_path {
                // Harness dump (WSP1L correlation): one JSONL line per
                // terminal trace. Written under the traces lock so lines
                // never interleave; a dump failure never fails the request.
                match serde_json::to_string(trace) {
                    Ok(line) => {
                        if let Err(err) = append_line(path, &line) {
                            tracing::warn!("interaction-trace dump failed: {err}");
                        }
                    }
                    Err(err) => {
                        tracing::warn!("interaction-trace dump encode failed: {err}");
                    }
                }
            }
        });
    }
}

/// Append one JSONL line to the dump file (creates it if the anchor write
/// was interrupted).
fn append_line(path: &std::path::Path, line: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")
}

impl Drop for TraceSpan<'_> {
    fn drop(&mut self) {
        // Cancellation or abandonment: no `complete` stamp, no blocking verdict.
        self.record_terminal(TraceStatus::Cancelled, false);
    }
}

/// First server stage whose gap to the next observed later stage dominates.
///
/// Computed over server-observed stamps only. The server cannot observe its
/// own transport write — `outbound_written` never appears in a server-side
/// snapshot (it is hydrated from client decode stamps when the harness
/// combines timelines) — so a missing `outbound_written` is NOT evidence of a
/// blocked outbound queue here. The stalled-reader rule (enqueued without
/// written) lives in the harness's combined-timeline analysis instead; applying
/// it to server-only stamps would blame the outbound queue for every healthy
/// traced request.
pub fn first_blocked_server_stage(stamps: &[StageStamp]) -> Option<ProtocolStage> {
    if stamps.is_empty() {
        return None;
    }
    let mut by_stage: Vec<(ProtocolStage, f64)> = Vec::new();
    for stamp in stamps {
        if let Some(existing) = by_stage.iter_mut().find(|(stage, _)| *stage == stamp.stage) {
            existing.1 = stamp.at_ms;
        } else {
            by_stage.push((stamp.stage, stamp.at_ms));
        }
    }
    by_stage.sort_by_key(|(stage, _)| stage.rank());

    const BLOCK_MS: f64 = 1.0;
    let mut worst: Option<(ProtocolStage, f64)> = None;
    for window in by_stage.windows(2) {
        let gap = window[1].1 - window[0].1;
        if gap < BLOCK_MS {
            continue;
        }
        match worst {
            Some((_, worst_gap)) if gap <= worst_gap => {}
            _ => worst = Some((window[0].0, gap)),
        }
    }
    worst.map(|(stage, _)| stage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn disabled_records_nothing() {
        let log = InteractionTraceLog::new(8);
        {
            let span = log.begin("textDocument/hover", || Some(3));
            span.mark(ProtocolStage::ProviderWork);
        }
        let snap = log.snapshot();
        assert!(!snap.enabled);
        assert!(snap.traces.is_empty());
        assert!(snap.unavailable_metrics.contains(&"client_input_to_paint"));
        assert!(snap
            .unavailable_metrics
            .contains(&"language_service_incremental_rss"));
    }

    #[test]
    fn records_request_and_source_epoch_without_source_text() {
        let log = InteractionTraceLog::new(8);
        log.set_enabled(true);
        {
            let span = log.begin("textDocument/hover", || Some(11));
            span.mark_with_bytes(ProtocolStage::Serialize, Some(128));
            span.mark(ProtocolStage::OutboundEnqueued);
            span.finish_ok();
        }
        let snap = log.snapshot();
        assert_eq!(snap.traces.len(), 1);
        let trace = &snap.traces[0];
        assert_eq!(trace.request_epoch, 1);
        assert_eq!(trace.source_epoch, Some(11));
        assert_eq!(trace.method, "textDocument/hover");
        assert_eq!(trace.status, TraceStatus::Complete);
        let json = serde_json::to_string(trace).expect("trace serializes");
        assert!(!json.contains("sourceText"));
        assert!(!json.contains("fileContents"));
        assert!(json.contains("requestEpoch"));
    }

    #[test]
    fn healthy_request_without_write_observation_has_no_blocked_stage() {
        // Production stamps never include `outbound_written` (the transport
        // write is invisible to the service layer), so a fast healthy request
        // must not blame the outbound queue — the missing-written rule belongs
        // to the harness's combined timelines, not server-only stamps.
        let stamps = vec![
            StageStamp {
                stage: ProtocolStage::RequestReceived,
                at_ms: 1.0,
                byte_length: None,
            },
            StageStamp {
                stage: ProtocolStage::ProviderWork,
                at_ms: 1.2,
                byte_length: None,
            },
            StageStamp {
                stage: ProtocolStage::Serialize,
                at_ms: 1.3,
                byte_length: Some(64),
            },
            StageStamp {
                stage: ProtocolStage::OutboundEnqueued,
                at_ms: 1.4,
                byte_length: Some(64),
            },
            StageStamp {
                stage: ProtocolStage::Complete,
                at_ms: 1.5,
                byte_length: None,
            },
        ];
        assert_eq!(first_blocked_server_stage(&stamps), None);
    }

    #[test]
    fn provider_work_dominates_when_it_is_the_long_gap() {
        // Stamps are stage *starts*. The 40ms dwell is inside provider_work.
        // No `outbound_written`: the server never observes its own write.
        let stamps = vec![
            StageStamp {
                stage: ProtocolStage::RequestReceived,
                at_ms: 0.0,
                byte_length: None,
            },
            StageStamp {
                stage: ProtocolStage::Admitted,
                at_ms: 0.1,
                byte_length: None,
            },
            StageStamp {
                stage: ProtocolStage::ProviderWork,
                at_ms: 0.2,
                byte_length: None,
            },
            StageStamp {
                stage: ProtocolStage::Serialize,
                at_ms: 40.2,
                byte_length: Some(16),
            },
            StageStamp {
                stage: ProtocolStage::OutboundEnqueued,
                at_ms: 40.3,
                byte_length: Some(16),
            },
            StageStamp {
                stage: ProtocolStage::Complete,
                at_ms: 40.5,
                byte_length: None,
            },
        ];
        assert_eq!(
            first_blocked_server_stage(&stamps),
            Some(ProtocolStage::ProviderWork)
        );
    }

    #[test]
    fn payload_growth_is_serialize_not_provider_work() {
        // Large payload dwells in serialize, not in query work.
        let stamps = vec![
            StageStamp {
                stage: ProtocolStage::RequestReceived,
                at_ms: 0.0,
                byte_length: None,
            },
            StageStamp {
                stage: ProtocolStage::ProviderWork,
                at_ms: 0.2,
                byte_length: None,
            },
            StageStamp {
                stage: ProtocolStage::Serialize,
                at_ms: 0.3,
                byte_length: Some(1_000_000),
            },
            StageStamp {
                stage: ProtocolStage::OutboundEnqueued,
                at_ms: 12.0,
                byte_length: Some(1_000_000),
            },
            StageStamp {
                stage: ProtocolStage::Complete,
                at_ms: 12.5,
                byte_length: None,
            },
        ];
        assert_eq!(
            first_blocked_server_stage(&stamps),
            Some(ProtocolStage::Serialize)
        );
    }

    #[test]
    fn finish_ok_records_complete_and_blocking() {
        let log = InteractionTraceLog::new(8);
        log.set_enabled(true);
        {
            let span = log.begin("textDocument/definition", || None);
            span.mark(ProtocolStage::Admitted);
            let spin = Instant::now();
            while spin.elapsed() < Duration::from_millis(2) {
                std::hint::spin_loop();
            }
            span.finish_ok();
        }
        let trace = &log.snapshot().traces[0];
        assert_eq!(trace.status, TraceStatus::Complete);
        assert!(trace
            .stamps
            .iter()
            .any(|stamp| stamp.stage == ProtocolStage::Complete));
        assert!(trace.stamps[0].at_ms <= trace.stamps.last().unwrap().at_ms);
    }

    #[test]
    fn fail_records_failed_without_complete_stamp() {
        let log = InteractionTraceLog::new(8);
        log.set_enabled(true);
        {
            let span = log.begin("textDocument/hover", || None);
            span.mark(ProtocolStage::ProviderWork);
            span.fail();
        }
        let trace = &log.snapshot().traces[0];
        assert_eq!(trace.status, TraceStatus::Failed);
        assert!(!trace
            .stamps
            .iter()
            .any(|stamp| stamp.stage == ProtocolStage::Complete));
        assert_eq!(trace.first_blocked_stage, None);
    }

    #[test]
    fn dropped_live_span_is_cancelled_not_complete() {
        let log = InteractionTraceLog::new(8);
        log.set_enabled(true);
        {
            let span = log.begin("textDocument/hover", || None);
            span.mark(ProtocolStage::ProviderWork);
            drop(span);
        }
        let trace = &log.snapshot().traces[0];
        assert_eq!(trace.status, TraceStatus::Cancelled);
        assert!(!trace
            .stamps
            .iter()
            .any(|stamp| stamp.stage == ProtocolStage::Complete));
        assert_eq!(trace.first_blocked_stage, None);
    }

    #[test]
    fn in_flight_request_snapshots_as_pending() {
        let log = InteractionTraceLog::new(8);
        log.set_enabled(true);
        let span = log.begin("textDocument/hover", || None);
        assert_eq!(log.snapshot().traces[0].status, TraceStatus::Pending);
        span.finish_ok();
    }
}
