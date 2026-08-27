//! Thread-local phase hooks used only by deterministic resolver concurrency
//! tests. The module and every call site are absent from non-test builds.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ResolutionPhase {
    ExactTableLookup,
    ProjectSelection,
    FilesystemProbing,
    ProviderProjection,
    PreAdmissionValidation,
    ParsedEdgePreCommit,
    RequestCompletion,
}

struct InstalledHook {
    phase: ResolutionPhase,
    action: Option<Box<dyn FnMut()>>,
    repeat: bool,
}

thread_local! {
    static HOOK: RefCell<Option<InstalledHook>> = const { RefCell::new(None) };
    static WORLD_CONTRACT: RefCell<Option<WorldContractState>> = const { RefCell::new(None) };
    static COMPLETED_OUTPUTS_AT_FINAL_FENCE: Cell<Option<usize>> = const { Cell::new(None) };
    static COMPLETED_OUTPUTS_AT_PUBLICATION: Cell<Option<usize>> = const { Cell::new(None) };
}

pub(crate) fn record_completed_outputs_at_final_fence(count: usize) {
    COMPLETED_OUTPUTS_AT_FINAL_FENCE.set(Some(count));
}

pub(crate) fn take_completed_outputs_at_final_fence() -> Option<usize> {
    COMPLETED_OUTPUTS_AT_FINAL_FENCE.take()
}

pub(crate) fn record_completed_outputs_at_publication(count: usize) {
    COMPLETED_OUTPUTS_AT_PUBLICATION.set(Some(count));
}

pub(crate) fn take_completed_outputs_at_publication() -> Option<usize> {
    COMPLETED_OUTPUTS_AT_PUBLICATION.take()
}

struct ClearHook;

impl Drop for ClearHook {
    fn drop(&mut self) {
        HOOK.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

pub(crate) fn with_hook<T>(
    phase: ResolutionPhase,
    action: impl FnOnce() + 'static,
    operation: impl FnOnce() -> T,
) -> T {
    let mut action = Some(action);
    HOOK.with(|slot| {
        assert!(
            slot.borrow().is_none(),
            "resolution concurrency hooks must not nest"
        );
        *slot.borrow_mut() = Some(InstalledHook {
            phase,
            action: Some(Box::new(move || {
                action
                    .take()
                    .expect("a one-shot resolution hook must fire once")();
            })),
            repeat: false,
        });
    });
    let _clear = ClearHook;
    operation()
}

pub(crate) fn with_repeating_hook<T>(
    phase: ResolutionPhase,
    action: impl FnMut() + 'static,
    operation: impl FnOnce() -> T,
) -> T {
    HOOK.with(|slot| {
        assert!(
            slot.borrow().is_none(),
            "resolution concurrency hooks must not nest"
        );
        *slot.borrow_mut() = Some(InstalledHook {
            phase,
            action: Some(Box::new(action)),
            repeat: true,
        });
    });
    let _clear = ClearHook;
    operation()
}

pub(crate) fn fire(phase: ResolutionPhase) {
    let action = HOOK.with(|slot| {
        let mut slot = slot.borrow_mut();
        let installed = slot.as_mut()?;
        if installed.phase != phase {
            return None;
        }
        installed
            .action
            .take()
            .map(|action| (action, installed.repeat))
    });
    if let Some((mut action, repeat)) = action {
        action();
        if repeat {
            HOOK.with(|slot| {
                let mut slot = slot.borrow_mut();
                if let Some(installed) = slot.as_mut() {
                    installed.action = Some(action);
                }
            });
        }
    }
}

/// Test-double for the immutable fact-version root exposed to resolver tests.
///
/// The strings describe semantic observations, not production storage fields.
/// The concurrency assertions therefore survive the removal of the current
/// generation-stamped lazy-cache entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolutionWorldSignature {
    facts: BTreeSet<&'static str>,
}

impl ResolutionWorldSignature {
    pub(crate) fn from_facts<const N: usize>(facts: [&'static str; N]) -> Self {
        Self {
            facts: facts.into_iter().collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AdmissionWorlds {
    pub(crate) captured: ResolutionWorldSignature,
    pub(crate) validated: ResolutionWorldSignature,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolutionTransactionObservation {
    pub(crate) attempts: Vec<ResolutionWorldSignature>,
    pub(crate) final_world: ResolutionWorldSignature,
    pub(crate) admission: Option<ResolutionAdmissionObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolutionAdmissionObservation {
    Cacheable(AdmissionWorlds),
    ReturnOnly,
}

struct WorldContractState {
    current: ResolutionWorldSignature,
    attempts: Vec<ResolutionWorldSignature>,
    admission: Option<ResolutionAdmissionObservation>,
}

struct ClearWorldContract(bool);

impl Drop for ClearWorldContract {
    fn drop(&mut self) {
        if self.0 {
            WORLD_CONTRACT.with(|slot| {
                slot.borrow_mut().take();
            });
        }
    }
}

pub(crate) fn with_world_contract<T>(
    initial: ResolutionWorldSignature,
    operation: impl FnOnce() -> T,
) -> (T, ResolutionTransactionObservation) {
    WORLD_CONTRACT.with(|slot| {
        assert!(
            slot.borrow().is_none(),
            "resolution world-contract probes must not nest"
        );
        *slot.borrow_mut() = Some(WorldContractState {
            current: initial,
            attempts: Vec::new(),
            admission: None,
        });
    });
    let mut clear = ClearWorldContract(true);
    let value = operation();
    let state = WORLD_CONTRACT.with(|slot| {
        slot.borrow_mut()
            .take()
            .expect("world-contract state must remain installed")
    });
    clear.0 = false;
    (
        value,
        ResolutionTransactionObservation {
            attempts: state.attempts,
            final_world: state.current,
            admission: state.admission,
        },
    )
}

pub(crate) fn publish_world(signature: ResolutionWorldSignature) {
    WORLD_CONTRACT.with(|slot| {
        if let Some(state) = slot.borrow_mut().as_mut() {
            state.current = signature;
        }
    });
}

pub(crate) fn capture_attempt_world() {
    WORLD_CONTRACT.with(|slot| {
        if let Some(state) = slot.borrow_mut().as_mut() {
            state.attempts.push(state.current.clone());
        }
    });
}

pub(crate) fn record_cacheable_admission() {
    WORLD_CONTRACT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(state) = slot.as_mut() else {
            return;
        };
        let captured = state
            .attempts
            .last()
            .cloned()
            .expect("an admission must belong to a captured attempt");
        state.admission = Some(ResolutionAdmissionObservation::Cacheable(AdmissionWorlds {
            captured,
            validated: state.current.clone(),
        }));
    });
}

#[allow(dead_code)]
pub(crate) fn record_return_only() {
    WORLD_CONTRACT.with(|slot| {
        if let Some(state) = slot.borrow_mut().as_mut() {
            state.admission = Some(ResolutionAdmissionObservation::ReturnOnly);
        }
    });
}
