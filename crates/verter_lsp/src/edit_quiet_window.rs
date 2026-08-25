//! Domain-owned quiet-window policy for editor-edit silence.
//!
//! BOTH the [`crate::sync_coordinator`] debounce and the edit-triggered
//! import-dependency publication consume this one value. A second 300 ms
//! constant is a bug, not a coincidence.

use std::time::Duration;

/// Milliseconds of silence after the last edit before a debounced sync or
/// import-dependency republication fires.
pub(crate) const EDIT_QUIET_WINDOW_MS: u64 = 300;

/// Silence required after the last edit before a debounced sync or
/// import-dependency republication fires.
pub(crate) const EDIT_QUIET_WINDOW: Duration = Duration::from_millis(EDIT_QUIET_WINDOW_MS);
