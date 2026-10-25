//! Interior mutability for request state a suspended computation owns.
//!
//! A pollable computation that suspends while it holds a shared reference
//! to its own request state is `Send` only if that state is `Sync`. The
//! state is still driven by one poller at a time; these cells keep the
//! borrow discipline of [`std::cell::RefCell`] and [`std::cell::Cell`] — a
//! conflicting borrow panics, it never waits. Scalar cells use typed atomics
//! where supported; arbitrary `Copy` values use a mutex. The state can move
//! between polling threads with the computation.
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

    #[inline(always)]
    fn lock(&self) -> parking_lot::MutexGuard<'_, T> {
        #[cfg(test)]
        tests::LOCK_ACQUIRES.with(|count| count.set(count.get() + 1));
        self.0.lock()
    }

    pub fn get(&self) -> T {
        *self.lock()
    }

    pub fn set(&self, value: T) {
        *self.lock() = value;
    }

    /// Replace the value, returning the old one.
    pub fn replace(&self, value: T) -> T {
        std::mem::replace(&mut *self.lock(), value)
    }

    /// Take the value, leaving its default.
    pub fn take(&self) -> T
    where
        T: Default,
    {
        std::mem::take(&mut *self.lock())
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

// Each scalar uses its own typed atomic; values never pass through bytes or
// pointer-width storage. A target without the required width keeps SyncCell's
// mutex, including whole-value storage for u64 on narrower targets.
macro_rules! scalar_cell {
    ($name:ident, $value:ty, $atomic:ident, $width:literal) => {
        /// A scalar cell for exclusively driven request state.
        ///
        /// Reads acquire and writes release, preserving publication between
        /// polling threads. This does not make a read/modify/write sequence
        /// atomic: the owning computation must still have only one driver.
        /// On targets without this atomic width it uses [`SyncCell`].
        #[cfg(target_has_atomic = $width)]
        #[derive(Default)]
        pub struct $name(std::sync::atomic::$atomic);

        #[cfg(not(target_has_atomic = $width))]
        pub type $name = SyncCell<$value>;

        #[cfg(target_has_atomic = $width)]
        impl $name {
            pub const fn new(value: $value) -> Self {
                Self(std::sync::atomic::$atomic::new(value))
            }

            #[inline(always)]
            pub fn get(&self) -> $value {
                self.0.load(std::sync::atomic::Ordering::Acquire)
            }

            #[inline(always)]
            pub fn set(&self, value: $value) {
                self.0.store(value, std::sync::atomic::Ordering::Release);
            }

            /// Replace the value, returning the old one.
            pub fn replace(&self, value: $value) -> $value {
                self.0.swap(value, std::sync::atomic::Ordering::AcqRel)
            }

            pub fn take(&self) -> $value {
                self.replace(<$value>::default())
            }

            /// Exchange two cells; like `SyncCell::swap`, this is not a
            /// transaction over the pair and requires exclusive driving.
            pub fn swap(&self, other: &Self) {
                if !std::ptr::eq(self, other) {
                    self.set(other.replace(self.get()));
                }
            }

            pub fn get_mut(&mut self) -> &mut $value {
                self.0.get_mut()
            }

            pub fn into_inner(self) -> $value {
                self.0.into_inner()
            }
        }

        #[cfg(target_has_atomic = $width)]
        impl Clone for $name {
            fn clone(&self) -> Self {
                Self::new(self.get())
            }
        }

        #[cfg(target_has_atomic = $width)]
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_tuple(stringify!($name)).field(&self.get()).finish()
            }
        }
    };
}

scalar_cell!(SyncBool, bool, AtomicBool, "8");
scalar_cell!(SyncUsize, usize, AtomicUsize, "ptr");
scalar_cell!(SyncU16, u16, AtomicU16, "16");
scalar_cell!(SyncU32, u32, AtomicU32, "32");
scalar_cell!(SyncU64, u64, AtomicU64, "64");

#[cfg(test)]
mod tests {
    use super::*;

    thread_local! {
        pub(super) static LOCK_ACQUIRES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    #[test]
    fn supported_scalar_operations_do_not_acquire_mutexes() {
        LOCK_ACQUIRES.with(|count| count.set(0));
        macro_rules! check {
            ($cell:ident, $value:expr, $width:literal) => {
                #[cfg(target_has_atomic = $width)]
                {
                    let mut cell = $cell::new($value);
                    assert_eq!(cell.replace(Default::default()), $value);
                    cell.set($value);
                    assert_eq!(cell.get(), $value);
                    assert_eq!(cell.take(), $value);
                    *cell.get_mut() = $value;
                    let other = $cell::default();
                    cell.swap(&other);
                    other.swap(&other);
                    assert_eq!(other.clone().into_inner(), $value);
                    assert_eq!(cell.into_inner(), Default::default());
                }
            };
        }
        check!(SyncBool, true, "8");
        check!(SyncUsize, usize::MAX, "ptr");
        check!(SyncU16, u16::MAX, "16");
        check!(SyncU32, u32::MAX, "32");
        check!(SyncU64, u64::MAX, "64");
        let locks = LOCK_ACQUIRES.with(|count| count.get());
        eprintln!("scalar_lock_acquires={locks}");
        assert_eq!(locks, 0);
    }

    #[test]
    fn scalar_state_resumes_on_another_thread_and_wide_reads_are_whole() {
        let value = SyncU64::new(0xaaaa_aaaa_aaaa_aaaa);
        let ready = SyncBool::new(false);
        let progress = SyncUsize::new(0);
        std::thread::scope(|scope| {
            scope.spawn(|| {
                for index in 0..10_000 {
                    value.set(if index % 2 == 0 {
                        0x5555_5555_5555_5555
                    } else {
                        0xaaaa_aaaa_aaaa_aaaa
                    });
                    progress.set(index + 1);
                }
                ready.set(true);
            });
            // Reads may overlap the producer; each wide value must stay whole.
            for _ in 0..10_000 {
                assert!(matches!(
                    value.get(),
                    0x5555_5555_5555_5555 | 0xaaaa_aaaa_aaaa_aaaa
                ));
                if ready.get() {
                    assert_eq!(progress.get(), 10_000);
                }
            }
        });
        std::thread::spawn(move || {
            assert!(ready.get());
            assert_eq!(progress.replace(0), 10_000);
            assert_eq!(value.take(), 0xaaaa_aaaa_aaaa_aaaa);
        })
        .join()
        .unwrap();
    }

    #[test]
    fn arbitrary_copy_storage_preserves_padding_and_wide_values() {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        struct Record {
            flag: bool,
            wide: u128,
        }
        let first = Record {
            flag: true,
            wide: u128::MAX,
        };
        let second = Record {
            flag: false,
            wide: 1,
        };
        let cell = SyncCell::new(first);
        assert_eq!(cell.replace(second), first);
        assert_eq!(cell.into_inner(), second);
    }

    #[test]
    fn shared_borrows_nest_and_a_conflicting_borrow_panics() {
        let cell = SyncRefCell::new(vec![1]);
        {
            let first = cell.borrow();
            let second = cell.borrow();
            assert_eq!((first.len(), second.len()), (1, 1));
            assert_eq!(cell.try_borrow_mut().err(), Some(AlreadyBorrowed));
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _held = cell.borrow_mut();
            }))
            .is_err());
        }
        cell.borrow_mut().push(2);
        {
            let _held = cell.borrow_mut();
            assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _held = cell.borrow();
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
        sync::<SyncBool>();
        sync::<SyncUsize>();
        sync::<SyncU16>();
        sync::<SyncU32>();
        sync::<SyncU64>();
    }
}
