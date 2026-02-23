//! Feature-gated concurrency primitive: `RwLock` (default) or `RefCell` (`single_threaded`).

#[cfg(feature = "single_threaded")]
pub(crate) type Shared<T> = std::cell::RefCell<T>;
#[cfg(not(feature = "single_threaded"))]
pub(crate) type Shared<T> = std::sync::RwLock<T>;

#[cfg(feature = "single_threaded")]
pub(crate) fn read_lock<T>(lock: &Shared<T>) -> std::cell::Ref<'_, T> {
    lock.borrow()
}
#[cfg(not(feature = "single_threaded"))]
pub(crate) fn read_lock<T>(lock: &Shared<T>) -> std::sync::RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|e| e.into_inner())
}

#[cfg(feature = "single_threaded")]
pub(crate) fn write_lock<T>(lock: &Shared<T>) -> std::cell::RefMut<'_, T> {
    lock.borrow_mut()
}
#[cfg(not(feature = "single_threaded"))]
pub(crate) fn write_lock<T>(lock: &Shared<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn default_shared<T>(value: T) -> Shared<T> {
    #[cfg(feature = "single_threaded")]
    {
        std::cell::RefCell::new(value)
    }
    #[cfg(not(feature = "single_threaded"))]
    {
        std::sync::RwLock::new(value)
    }
}
