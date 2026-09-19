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

/// One request's timeline, keyed by request epoch and optional source epoch.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionTrace {
    pub request_epoch: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_epoch: Option<u64>,
    pub method: String,
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
    traces: Mutex<Vec<InteractionTrace>>,
    max_traces: usize,
}

impl InteractionTraceLog {
    pub fn new(max_traces: usize) -> Self {
        Self {
            enabled: AtomicBool::new(false),
            next_epoch: AtomicU64::new(1),
            session_start: Instant::now(),
            traces: Mutex::new(Vec::new()),
            max_traces,
        }
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

    /// Open a request span. Returns a no-op span when disabled.
    pub fn begin(&self, method: &str, source_epoch: Option<u64>) -> TraceSpan<'_> {
        if !self.is_enabled() {
            return TraceSpan {
                log: self,
                epoch: 0,
                live: false,
            };
        }
        let epoch = self.next_epoch.fetch_add(1, Ordering::Relaxed);
        let trace = InteractionTrace {
            request_epoch: epoch,
            source_epoch,
            method: method.to_string(),
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

/// RAII span for one request. Drop records `complete` and the first blocked stage.
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

    pub fn finish(self) {
        drop(self);
    }
}

impl Drop for TraceSpan<'_> {
    fn drop(&mut self) {
        if !self.live {
            return;
        }
        let at_ms = self.log.now_ms();
        self.log.with_trace(self.epoch, |trace| {
            if !trace
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
            trace.first_blocked_stage = first_blocked_server_stage(&trace.stamps);
        });
    }
}

/// First server stage whose gap to the next observed later stage dominates.
///
/// Missing `outbound_written` after `outbound_enqueued` is treated as a
/// blocked outbound queue (stalled reader) even when `complete` is immediate.
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

    let has_enqueued = by_stage
        .iter()
        .any(|(stage, _)| *stage == ProtocolStage::OutboundEnqueued);
    let has_written = by_stage
        .iter()
        .any(|(stage, _)| *stage == ProtocolStage::OutboundWritten);
    if has_enqueued && !has_written {
        return Some(ProtocolStage::OutboundEnqueued);
    }

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
            let span = log.begin("textDocument/hover", Some(3));
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
            let span = log.begin("textDocument/hover", Some(11));
            span.mark_with_bytes(ProtocolStage::Serialize, Some(128));
            span.mark(ProtocolStage::OutboundEnqueued);
            span.mark(ProtocolStage::OutboundWritten);
        }
        let snap = log.snapshot();
        assert_eq!(snap.traces.len(), 1);
        let trace = &snap.traces[0];
        assert_eq!(trace.request_epoch, 1);
        assert_eq!(trace.source_epoch, Some(11));
        assert_eq!(trace.method, "textDocument/hover");
        let json = serde_json::to_string(trace).expect("trace serializes");
        assert!(!json.contains("sourceText"));
        assert!(!json.contains("fileContents"));
        assert!(json.contains("requestEpoch"));
    }

    #[test]
    fn stalled_outbound_is_first_blocked_even_when_complete_is_immediate() {
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
        assert_eq!(
            first_blocked_server_stage(&stamps),
            Some(ProtocolStage::OutboundEnqueued)
        );
    }

    #[test]
    fn provider_work_dominates_when_it_is_the_long_gap() {
        // Stamps are stage *starts*. The 40ms dwell is inside provider_work.
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
                stage: ProtocolStage::OutboundWritten,
                at_ms: 40.4,
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
                stage: ProtocolStage::OutboundWritten,
                at_ms: 12.4,
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
    fn drop_finalizes_complete() {
        let log = InteractionTraceLog::new(8);
        log.set_enabled(true);
        {
            let span = log.begin("textDocument/definition", None);
            span.mark(ProtocolStage::Admitted);
            let spin = Instant::now();
            while spin.elapsed() < Duration::from_millis(2) {
                std::hint::spin_loop();
            }
        }
        let trace = &log.snapshot().traces[0];
        assert!(trace
            .stamps
            .iter()
            .any(|stamp| stamp.stage == ProtocolStage::Complete));
        assert!(trace.stamps[0].at_ms <= trace.stamps.last().unwrap().at_ms);
    }
}
