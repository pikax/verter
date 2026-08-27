//! Compact bump storage for [`crate::StyleSyntaxIr`] / selector trees.
//!
//! Child lists live in a single [`bumpalo::Bump`] owned by the parent IR. The
//! public accessors still return `&[T]`; the pointer is valid for the bump's
//! lifetime. Public bump-backed nodes are not `Clone` or `Copy`, so a caller
//! cannot take an owned handle that outlives [`crate::StyleSyntaxIr`] /
//! [`crate::SelectorStructure`]. Clone the IR (`Arc`) when an owned handle is
//! needed; that clone keeps the bump alive. `BumpSlice` / `BumpStr` themselves
//! stay crate-private `Copy` handles.

use std::fmt;
use std::marker::PhantomData;

use bumpalo::Bump;

/// Slice allocated in the parent IR bump. Copying it does not copy `T`.
pub(crate) struct BumpSlice<T> {
    ptr: *const T,
    len: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Copy for BumpSlice<T> {}
impl<T> Clone for BumpSlice<T> {
    fn clone(&self) -> Self {
        *self
    }
}

unsafe impl<T: Send> Send for BumpSlice<T> {}
unsafe impl<T: Sync> Sync for BumpSlice<T> {}

impl<T> BumpSlice<T> {
    pub(crate) const fn empty() -> Self {
        Self {
            ptr: std::ptr::NonNull::dangling().as_ptr(),
            len: 0,
            _marker: PhantomData,
        }
    }

    pub(crate) fn from_slice(slice: &[T]) -> Self {
        if slice.is_empty() {
            return Self::empty();
        }
        Self {
            ptr: slice.as_ptr(),
            len: u32::try_from(slice.len()).unwrap_or(u32::MAX),
            _marker: PhantomData,
        }
    }

    #[inline]
    pub(crate) fn as_slice(&self) -> &[T] {
        if self.len == 0 {
            return &[];
        }
        unsafe { std::slice::from_raw_parts(self.ptr, self.len as usize) }
    }

    #[inline]
    pub(crate) const fn len(self) -> usize {
        self.len as usize
    }
}

impl<T: PartialEq> PartialEq for BumpSlice<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Eq> Eq for BumpSlice<T> {}

impl<T: fmt::Debug> fmt::Debug for BumpSlice<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_slice().fmt(f)
    }
}

/// UTF-8 text allocated in the parent IR bump.
#[derive(Copy, Clone)]
pub(crate) struct BumpStr {
    ptr: *const u8,
    len: u32,
}

unsafe impl Send for BumpStr {}
unsafe impl Sync for BumpStr {}

impl BumpStr {
    pub(crate) const fn empty() -> Self {
        Self {
            ptr: std::ptr::NonNull::dangling().as_ptr(),
            len: 0,
        }
    }

    pub(crate) fn from_str(text: &str) -> Self {
        if text.is_empty() {
            return Self::empty();
        }
        Self {
            ptr: text.as_ptr(),
            len: u32::try_from(text.len()).unwrap_or(u32::MAX),
        }
    }

    #[inline]
    pub(crate) fn as_str(&self) -> &str {
        if self.len == 0 {
            return "";
        }
        unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(self.ptr, self.len as usize))
        }
    }
}

impl PartialEq for BumpStr {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for BumpStr {}

impl fmt::Debug for BumpStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}

pub(crate) fn freeze_vec<T>(vec: bumpalo::collections::Vec<T>) -> BumpSlice<T> {
    BumpSlice::from_slice(vec.into_bump_slice())
}

pub(crate) fn alloc_str(bump: &Bump, text: &str) -> BumpStr {
    if text.is_empty() {
        return BumpStr::empty();
    }
    BumpStr::from_str(bump.alloc_str(text))
}

/// One bump chunk sized from the source, so a typical stylesheet stays inside a
/// single global allocation. Linear in source length so per-rule requested
/// bytes stay constant as the stylesheet grows.
pub(crate) fn bump_for_source(source_len: usize) -> Bump {
    Bump::with_capacity(source_len.saturating_mul(32).max(2048))
}
