//! Concurrency primitives for the host's internal state.

use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

pub(crate) type Shared<T> = RwLock<T>;

pub(crate) fn read_lock<T>(lock: &Shared<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn write_lock<T>(lock: &Shared<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn default_shared<T>(value: T) -> Shared<T> {
    RwLock::new(value)
}
