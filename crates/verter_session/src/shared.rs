//! Concurrency primitives for the host's internal state.
//!
//! On native targets, uses `parking_lot::RwLock` which is writer-preferring:
//! once a writer is waiting, new readers queue behind it. This prevents the
//! writer starvation that occurs with `std::sync::RwLock` on Windows (SRWLock
//! is reader-preferring).
//!
//! On WASM, uses `std::sync::RwLock` (single-threaded anyway, no contention).

// ── Native: parking_lot (writer-preferring) ──────────────────────────

#[cfg(not(target_arch = "wasm32"))]
pub(crate) type Shared<T> = parking_lot::RwLock<T>;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn read_lock<T>(lock: &Shared<T>) -> parking_lot::RwLockReadGuard<'_, T> {
    lock.read()
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn write_lock<T>(lock: &Shared<T>) -> parking_lot::RwLockWriteGuard<'_, T> {
    lock.write()
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn default_shared<T>(value: T) -> Shared<T> {
    parking_lot::RwLock::new(value)
}

// ── WASM: std::sync (single-threaded, no contention) ────────────────

#[cfg(target_arch = "wasm32")]
pub(crate) type Shared<T> = std::sync::RwLock<T>;

#[cfg(target_arch = "wasm32")]
pub(crate) fn read_lock<T>(lock: &Shared<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|e| e.into_inner())
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn write_lock<T>(lock: &Shared<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|e| e.into_inner())
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn default_shared<T>(value: T) -> Shared<T> {
    std::sync::RwLock::new(value)
}
