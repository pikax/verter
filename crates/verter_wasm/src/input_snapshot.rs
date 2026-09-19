//! Committed input-snapshot handoff for the browser host.
//!
//! The browser acquires file bytes asynchronously OUTSIDE any semantic
//! callback (worker message, OPFS, File System Access), then hands the
//! acquired rows to [`commit_input_snapshot_core`] in one synchronous
//! call. The core commits ONE immutable input basis through the session
//! handoff seam and stores it under its basis id; [`observe_input_snapshot_core`]
//! answers later synchronous observations from the committed rows only.
//!
//! A requested key that was never acquired answers the typed
//! `NeedInputs` status naming the canonical — the demand for the next
//! asynchronous acquisition wave. Neither entry reads the network, the
//! disk, or the workspace: the core has no acquisition capability, so
//! no synchronous fetch can happen inside a resolver callback.

use std::collections::HashMap;
use std::sync::Arc;

use verter_session::input_handoff::{AcquiredFile, CommittedInputHandoff, HandoffObserve};

/// One committed snapshot stored on the host.
#[derive(Clone)]
pub(crate) struct StoredInputSnapshot {
    handoff: Arc<CommittedInputHandoff>,
}

impl StoredInputSnapshot {
    /// Observe one canonical through the committed basis.
    pub(crate) fn observe(&self, canonical: &str) -> InputSnapshotObservationCore {
        match self.handoff.observe(canonical) {
            HandoffObserve::File { content, .. } => InputSnapshotObservationCore::File {
                content: content.to_string(),
            },
            HandoffObserve::Absent { .. } => InputSnapshotObservationCore::Absent,
            HandoffObserve::NeedInputs { .. } => InputSnapshotObservationCore::NeedInputs,
        }
    }

    /// Committed basis id as 64-char lowercase hex — the handle the
    /// browser holds between calls.
    pub(crate) fn basis_id_hex(&self) -> String {
        self.handoff.basis().id().digest().to_hex()
    }

    /// Committed positive file rows in this snapshot.
    pub(crate) fn file_count(&self) -> usize {
        self.handoff.basis().observations().count()
    }

    /// Committed probed-missing negatives in this snapshot.
    pub(crate) fn missing_count(&self) -> usize {
        self.handoff.basis().negatives().count()
    }
}

/// Typed wire observation outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputSnapshotObservationCore {
    /// Committed file bytes.
    File { content: String },
    /// The acquisition wave probed this key and recorded it absent.
    Absent,
    /// The key was never acquired: typed demand for the next
    /// asynchronous acquisition wave.
    NeedInputs,
}

/// Why a commit or observe was refused at the boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputSnapshotError {
    /// The acquisition wave was incoherent (duplicate canonical with
    /// different bytes, or a canonical both acquired and probed
    /// missing). The canonical constructor's own refusal.
    IncoherentWave(String),
    /// No committed snapshot exists under the presented basis id.
    UnknownBasisId,
    /// The store's lock was poisoned by a panic in an unrelated call.
    /// Unreachable in practice — commits return `Result` and never
    /// panic — but a lock failure is a refusal, never a silent read.
    StorePoisoned,
}

impl std::fmt::Display for InputSnapshotError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IncoherentWave(detail) => {
                write!(f, "committed input snapshot refused: {detail}")
            }
            Self::UnknownBasisId => {
                write!(f, "unknown committed input snapshot basis id")
            }
            Self::StorePoisoned => {
                write!(f, "committed input snapshot store unavailable")
            }
        }
    }
}

/// Commit one acquisition wave and store it under its basis id.
///
/// Pure handoff: no workspace write, no source registration, no
/// network. Registering sources for compilation stays on the existing
/// `upsert` route; this snapshot is the committed-input record those
/// sources were acquired under.
pub(crate) fn commit_input_snapshot_core(
    files: Vec<(String, String)>,
    missing: Vec<String>,
) -> Result<StoredInputSnapshot, InputSnapshotError> {
    let acquired = files
        .into_iter()
        .map(|(canonical, content)| AcquiredFile {
            canonical: Arc::from(canonical.as_str()),
            content: Arc::from(content.as_str()),
        })
        .collect::<Vec<_>>();
    let missing = missing
        .into_iter()
        .map(|canonical| Arc::from(canonical.as_str()))
        .collect::<Vec<_>>();
    let handoff = CommittedInputHandoff::commit(acquired, missing)
        .map_err(|error| InputSnapshotError::IncoherentWave(format!("{error:?}")))?;
    Ok(StoredInputSnapshot {
        handoff: Arc::new(handoff),
    })
}

/// The host-side snapshot registry: basis id → committed snapshot.
/// Latest commit wins nothing — every basis id stays addressable until
/// the host is dropped, so a browser tab holding an old id can never
/// observe another tab's newer basis through it.
pub(crate) struct InputSnapshotStore {
    by_basis_id: HashMap<String, StoredInputSnapshot>,
}

impl InputSnapshotStore {
    pub(crate) fn new() -> Self {
        Self {
            by_basis_id: HashMap::new(),
        }
    }

    /// Commit and index one wave; returns the stored snapshot.
    pub(crate) fn commit(
        &mut self,
        files: Vec<(String, String)>,
        missing: Vec<String>,
    ) -> Result<StoredInputSnapshot, InputSnapshotError> {
        let stored = commit_input_snapshot_core(files, missing)?;
        self.by_basis_id
            .insert(stored.basis_id_hex(), stored.clone());
        Ok(stored)
    }

    /// Observe `canonical` under the committed basis `basis_id_hex`.
    pub(crate) fn observe(
        &self,
        basis_id_hex: &str,
        canonical: &str,
    ) -> Result<InputSnapshotObservationCore, InputSnapshotError> {
        let stored = self
            .by_basis_id
            .get(basis_id_hex)
            .ok_or(InputSnapshotError::UnknownBasisId)?;
        Ok(stored.observe(canonical))
    }
}

impl Default for InputSnapshotStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// A requested missing file answers typed NeedInputs — never a
    /// fetch: the boundary holds no acquisition capability, and a key
    /// the wave probed-and-recorded missing answers Absent instead,
    /// distinct from NeedInputs.
    #[test]
    fn missing_and_unacquired_keys_answer_distinct_typed_statuses() {
        let mut store = InputSnapshotStore::new();
        let stored = store
            .commit(
                vec![("/probe.ts".to_string(), "export const a = 1;".to_string())],
                vec!["/gone.ts".to_string()],
            )
            .expect("coherent wave");

        let basis_id = stored.basis_id_hex();
        assert_eq!(
            store.observe(&basis_id, "/probe.ts"),
            Ok(InputSnapshotObservationCore::File {
                content: "export const a = 1;".to_string()
            })
        );
        assert_eq!(
            store.observe(&basis_id, "/gone.ts"),
            Ok(InputSnapshotObservationCore::Absent)
        );
        assert_eq!(
            store.observe(&basis_id, "/never-acquired.ts"),
            Ok(InputSnapshotObservationCore::NeedInputs)
        );
    }

    /// An incoherent wave is refused closed and stores nothing.
    #[test]
    fn incoherent_wave_is_refused_and_stores_nothing() {
        let mut store = InputSnapshotStore::new();
        let refused = store.commit(
            vec![
                ("/x.ts".to_string(), "1".to_string()),
                ("/x.ts".to_string(), "2".to_string()),
            ],
            vec![],
        );
        assert!(matches!(
            refused,
            Err(InputSnapshotError::IncoherentWave(_))
        ));
        assert!(store.observe("whatever", "/x.ts").is_err());
    }

    /// Identical committed waves produce identical basis ids — the
    /// same-basis equivalence native and browser executions rely on —
    /// and an unknown basis id is refused by name.
    #[test]
    fn identical_waves_share_one_basis_identity() {
        let mut store = InputSnapshotStore::new();
        let wave = || {
            (
                vec![("/probe.ts".to_string(), "export const a = 1;".to_string())],
                vec!["/gone.ts".to_string()],
            )
        };
        let (files, missing) = wave();
        let first = store.commit(files, missing).expect("coherent wave");
        let (files, missing) = wave();
        let second = store.commit(files, missing).expect("coherent wave");
        assert_eq!(first.basis_id_hex(), second.basis_id_hex());

        assert_eq!(
            store.observe("00", "/probe.ts").unwrap_err().to_string(),
            "unknown committed input snapshot basis id"
        );
    }
}
