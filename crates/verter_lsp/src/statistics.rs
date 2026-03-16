use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// Tracks per-operation timing statistics for the LSP session.
///
/// When disabled (the default), all recording operations are no-ops with zero overhead.
/// Enable via [`Statistics::set_enabled`] after reading initialization options.
pub struct Statistics {
    enabled: AtomicBool,
    events: Mutex<VecDeque<StatisticsEvent>>,
    max_events: usize,
    session_start: Instant,
}

/// A single recorded timing event.
#[derive(Debug, Clone)]
pub struct StatisticsEvent {
    pub event_type: String,
    pub uri: Option<String>,
    pub duration_ms: f64,
    /// Milliseconds since session start.
    pub started_at_ms: f64,
}

/// Aggregated summary for a group of events.
#[derive(Debug, Clone)]
pub struct StatisticsSummary {
    pub count: u64,
    pub total_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
}

impl StatisticsSummary {
    pub fn average_ms(&self) -> f64 {
        if self.count == 0 {
            0.0
        } else {
            self.total_ms / self.count as f64
        }
    }

    fn new() -> Self {
        Self {
            count: 0,
            total_ms: 0.0,
            min_ms: f64::MAX,
            max_ms: 0.0,
        }
    }

    fn record(&mut self, duration_ms: f64) {
        self.count += 1;
        self.total_ms += duration_ms;
        if duration_ms < self.min_ms {
            self.min_ms = duration_ms;
        }
        if duration_ms > self.max_ms {
            self.max_ms = duration_ms;
        }
    }
}

impl Statistics {
    /// Create a new statistics collector. Starts disabled with no events.
    pub fn new(max_events: usize) -> Self {
        Self {
            enabled: AtomicBool::new(false),
            events: Mutex::new(VecDeque::new()),
            max_events,
            session_start: Instant::now(),
        }
    }

    /// Check if statistics collection is enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    /// Enable or disable statistics collection.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// Record a timing event. Returns immediately if disabled.
    pub fn record(&self, event_type: &str, uri: Option<&str>, duration: Duration) {
        if !self.is_enabled() {
            return;
        }
        let event = StatisticsEvent {
            event_type: event_type.to_string(),
            uri: uri.map(|s| s.to_string()),
            duration_ms: duration.as_secs_f64() * 1000.0,
            started_at_ms: self.session_start.elapsed().as_secs_f64() * 1000.0,
        };
        let mut events = self.events.lock();
        if events.len() >= self.max_events {
            events.pop_front();
        }
        events.push_back(event);
    }

    /// Create a scoped timer that records the event when dropped.
    /// Returns `None` if statistics are disabled (zero overhead).
    pub fn timer(&self, event_type: &'static str, uri: Option<String>) -> Option<Timer<'_>> {
        if !self.is_enabled() {
            return None;
        }
        Some(Timer {
            stats: self,
            event_type,
            uri,
            start: Instant::now(),
        })
    }

    /// Summarize events grouped by event type.
    pub fn summary_by_type(&self) -> HashMap<String, StatisticsSummary> {
        let events = self.events.lock();
        let mut map = HashMap::new();
        for event in events.iter() {
            map.entry(event.event_type.clone())
                .or_insert_with(StatisticsSummary::new)
                .record(event.duration_ms);
        }
        // Fix min_ms for empty summaries
        for summary in map.values_mut() {
            if summary.count == 0 {
                summary.min_ms = 0.0;
            }
        }
        map
    }

    /// Summarize events grouped by file URI.
    pub fn summary_by_file(&self) -> HashMap<String, StatisticsSummary> {
        let events = self.events.lock();
        let mut map = HashMap::new();
        for event in events.iter() {
            if let Some(uri) = &event.uri {
                map.entry(uri.clone())
                    .or_insert_with(StatisticsSummary::new)
                    .record(event.duration_ms);
            }
        }
        for summary in map.values_mut() {
            if summary.count == 0 {
                summary.min_ms = 0.0;
            }
        }
        map
    }

    /// Get raw events (for `includeEvents: true` requests).
    pub fn events(&self) -> Vec<StatisticsEvent> {
        self.events.lock().iter().cloned().collect()
    }
}

/// RAII timer — records the event when dropped.
pub struct Timer<'a> {
    stats: &'a Statistics,
    event_type: &'static str,
    uri: Option<String>,
    start: Instant,
}

impl Drop for Timer<'_> {
    fn drop(&mut self) {
        let duration = self.start.elapsed();
        self.stats
            .record(self.event_type, self.uri.as_deref(), duration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    #[test]
    fn disabled_by_default() {
        let stats = Statistics::new(100);
        assert!(!stats.is_enabled());
        assert!(stats.timer("test", None).is_none());
    }

    #[test]
    fn records_events_when_enabled() {
        let stats = Statistics::new(100);
        stats.set_enabled(true);

        stats.record("parse", Some("file:///a.vue"), Duration::from_millis(10));
        stats.record("parse", Some("file:///b.vue"), Duration::from_millis(20));
        stats.record("compile", Some("file:///a.vue"), Duration::from_millis(5));

        let by_type = stats.summary_by_type();
        assert_eq!(by_type.len(), 2);

        let parse = &by_type["parse"];
        assert_eq!(parse.count, 2);
        assert!((parse.total_ms - 30.0).abs() < 1.0);
        assert!((parse.average_ms() - 15.0).abs() < 1.0);

        let by_file = stats.summary_by_file();
        assert_eq!(by_file.len(), 2);
        assert_eq!(by_file["file:///a.vue"].count, 2);
        assert_eq!(by_file["file:///b.vue"].count, 1);
    }

    #[test]
    fn fifo_eviction() {
        let stats = Statistics::new(3);
        stats.set_enabled(true);

        for i in 0..5 {
            stats.record("test", None, Duration::from_millis(i));
        }

        let events = stats.events();
        assert_eq!(events.len(), 3);
        // Oldest events evicted, newest retained
        assert!((events[0].duration_ms - 2.0).abs() < 0.1);
        assert!((events[1].duration_ms - 3.0).abs() < 0.1);
        assert!((events[2].duration_ms - 4.0).abs() < 0.1);
    }

    #[test]
    fn timer_records_on_drop() {
        let stats = Statistics::new(100);
        stats.set_enabled(true);

        {
            let _timer = stats.timer("slow_op", Some("file:///test.vue".to_string()));
            thread::sleep(Duration::from_millis(5));
        }

        let events = stats.events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "slow_op");
        assert_eq!(events[0].uri.as_deref(), Some("file:///test.vue"));
        assert!(events[0].duration_ms >= 4.0); // Allow some timing slack
    }

    #[test]
    fn no_events_when_disabled() {
        let stats = Statistics::new(100);
        // disabled by default
        stats.record("test", None, Duration::from_millis(10));
        assert!(stats.events().is_empty());
        assert!(stats.summary_by_type().is_empty());
    }
}
