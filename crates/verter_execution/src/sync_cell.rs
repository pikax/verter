//! Interior mutability for request state a suspended computation owns.
//!
//! A pollable computation that suspends while it holds a shared reference
//! to its own request state is `Send` only if that state is `Sync`. The
//! state is still driven by one poller at a time; these cells keep the
//! borrow discipline of [`std::cell::RefCell`] and [`std::cell::Cell`] — a
//! conflicting borrow panics, it never waits — with an atomic borrow flag,
//! so the state can move between polling threads with the computation.
//!
//! A borrow guard belongs to the thread that took it: holding one across a
//! suspension point makes the computation non-`Send`, so the compiler
//! rejects a borrow that would outlive its poll.

use std::fmt;

use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};

/// A [`std::cell::RefCell`] whose borrow flag is atomic.
///
/// `borrow` and `borrow_mut` follow `RefCell`'s rules on the thread that
/// polls the owning computation: shared borrows nest, and a borrow that
/// conflicts with a live one panics instead of waiting.
#[derive(Default)]
pub struct SyncRefCell<T>(RwLock<T>);

/// A [`SyncRefCell`] borrow refused because a conflicting borrow is live.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlreadyBorrowed;

/// A shared borrow of a [`SyncRefCell`].
pub type SyncRef<'a, T> = RwLockReadGuard<'a, T>;

/// An exclusive borrow of a [`SyncRefCell`].
pub type SyncRefMut<'a, T> = RwLockWriteGuard<'a, T>;

impl<T> SyncRefCell<T> {
    pub const fn new(value: T) -> Self {
        Self(parking_lot::const_rwlock(value))
    }

    /// Borrow the value shared.
    ///
    /// # Panics
    ///
    /// While the value is mutably borrowed.
    #[track_caller]
    pub fn borrow(&self) -> SyncRef<'_, T> {
        self.0
            .try_read_recursive()
            .expect("already mutably borrowed")
    }

    /// Borrow the value exclusively.
    ///
    /// # Panics
    ///
    /// While the value is borrowed.
    #[track_caller]
    pub fn borrow_mut(&self) -> SyncRefMut<'_, T> {
        self.0.try_write().expect("already borrowed")
    }

    /// Borrow the value exclusively, or fail while it is borrowed.
    pub fn try_borrow_mut(&self) -> Result<SyncRefMut<'_, T>, AlreadyBorrowed> {
        self.0.try_write().ok_or(AlreadyBorrowed)
    }

    /// Replace the value, returning the old one.
    ///
    /// # Panics
    ///
    /// While the value is borrowed.
    #[track_caller]
    pub fn replace(&self, value: T) -> T {
        std::mem::replace(&mut *self.borrow_mut(), value)
    }

    /// Take the value, leaving its default.
    ///
    /// # Panics
    ///
    /// While the value is borrowed.
    #[track_caller]
    pub fn take(&self) -> T
    where
        T: Default,
    {
        std::mem::take(&mut *self.borrow_mut())
    }

    /// Exchange the values of two cells.
    ///
    /// # Panics
    ///
    /// While either value is borrowed, or when both are the same cell.
    #[track_caller]
    pub fn swap(&self, other: &Self) {
        std::mem::swap(&mut *self.borrow_mut(), &mut *other.borrow_mut());
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.0.get_mut()
    }

    pub fn into_inner(self) -> T {
        self.0.into_inner()
    }
}

impl<T: Clone> Clone for SyncRefCell<T> {
    fn clone(&self) -> Self {
        Self::new(self.borrow().clone())
    }
}

impl<T: fmt::Debug> fmt::Debug for SyncRefCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.try_read_recursive() {
            Some(value) => f.debug_tuple("SyncRefCell").field(&*value).finish(),
            None => f.write_str("SyncRefCell(<borrowed>)"),
        }
    }
}

/// A [`std::cell::Cell`] of a `Copy` value that a `Sync` owner can hold.
#[derive(Default)]
pub struct SyncCell<T: Copy>(parking_lot::Mutex<T>);

impl<T: Copy> SyncCell<T> {
    pub const fn new(value: T) -> Self {
        Self(parking_lot::const_mutex(value))
    }

    pub fn get(&self) -> T {
        *self.0.lock()
    }

    pub fn set(&self, value: T) {
        *self.0.lock() = value;
    }

    /// Replace the value, returning the old one.
    pub fn replace(&self, value: T) -> T {
        std::mem::replace(&mut *self.0.lock(), value)
    }

    /// Take the value, leaving its default.
    pub fn take(&self) -> T
    where
        T: Default,
    {
        std::mem::take(&mut *self.0.lock())
    }

    /// Exchange the values of two cells.
    pub fn swap(&self, other: &Self) {
        if std::ptr::eq(self, other) {
            return;
        }
        let mine = self.get();
        self.set(other.replace(mine));
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.0.get_mut()
    }

    pub fn into_inner(self) -> T {
        self.0.into_inner()
    }
}

impl<T: Copy> Clone for SyncCell<T> {
    fn clone(&self) -> Self {
        Self::new(self.get())
    }
}

impl<T: Copy + fmt::Debug> fmt::Debug for SyncCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SyncCell").field(&self.get()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_borrows_nest_and_a_conflicting_borrow_panics() {
        let cell = SyncRefCell::new(vec![1]);
        {
            let first = cell.borrow();
            let second = cell.borrow();
            assert_eq!((first.len(), second.len()), (1, 1));
            assert_eq!(cell.try_borrow_mut().err(), Some(AlreadyBorrowed));
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = cell.borrow_mut();
            }))
            .is_err());
        }
        cell.borrow_mut().push(2);
        {
            let _held = cell.borrow_mut();
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = cell.borrow();
            }))
            .is_err());
        }
        assert_eq!(cell.replace(Vec::new()), [1, 2]);
        assert!(cell.take().is_empty());
    }

    #[test]
    fn a_cell_reads_back_what_it_was_given() {
        let cell = SyncCell::new(3_u32);
        assert_eq!(cell.replace(4), 3);
        cell.set(cell.get() + 1);
        assert_eq!(cell.take(), 5);
        assert_eq!(cell.get(), 0);
    }

    #[test]
    fn owners_of_the_cells_are_sync() {
        fn sync<T: Sync + Send>() {}
        sync::<SyncRefCell<Vec<u8>>>();
        sync::<SyncCell<u64>>();
    }
}
