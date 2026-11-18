//! Batches of changed paths released after a quiet window.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Collects distinct changed paths until the latest event has been quiet for
/// the configured window. Time is supplied by the caller.
pub struct Debouncer {
    quiet: Duration,
    pending: BTreeSet<PathBuf>,
    last_event: Option<Instant>,
}

impl Debouncer {
    #[must_use]
    pub fn new(quiet: Duration) -> Self {
        Self {
            quiet,
            pending: BTreeSet::new(),
            last_event: None,
        }
    }

    /// Record a change, including repeated paths. Older event timestamps never
    /// move the quiet window backwards.
    pub fn push(&mut self, path: PathBuf, at: Instant) {
        self.pending.insert(path);
        self.last_event = Some(self.last_event.map_or(at, |last| last.max(at)));
    }

    /// When the pending batch becomes ready: a quiet window past its latest
    /// event. `None` when no paths are pending.
    #[must_use]
    pub fn deadline(&self) -> Option<Instant> {
        self.last_event.map(|last| last + self.quiet)
    }

    /// Drain the batch once `now` is at least a quiet window past the latest
    /// event. No batch is released before that event or when no paths are pending.
    pub fn take_ready(&mut self, now: Instant) -> Option<Vec<PathBuf>> {
        if now.checked_duration_since(self.last_event?)? < self.quiet {
            return None;
        }

        self.last_event = None;
        Some(std::mem::take(&mut self.pending).into_iter().collect())
    }
}

#[cfg(test)]
#[path = "debounce_tests.rs"]
mod debounce_tests;
