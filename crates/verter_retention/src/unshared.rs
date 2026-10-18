//! Deep copies that share no raw allocation with their source.
//!
//! A data tree held in a self-charged cell ([`super::heap_size::Tree`])
//! keeps its raw `Arc`s inside the cell's one charge. A copy of such a tree
//! handed past the cell — a terminal unwrap of a retained payload — must not
//! alias those raw allocations, or the copy would keep them alive after the
//! cell's charge is released. [`Unshared::unshared`] builds a copy whose
//! every raw `Arc` is a fresh allocation the copy owns outright; a
//! self-charged owned handle ([`super::owned::Owned`] and the cells built on
//! it) is shared, since its charge lives in the allocation and follows every
//! owning clone.
//!
//! `#[derive(Unshared)]` copies every field of every variant.

use std::sync::Arc;

pub use verter_retention_derive::Unshared;

use super::heap_size::{HeapSize, OwnedValue};
use super::owned::{Measured, Owned, OwnedSlice, OwnedStr};

/// A value that can be copied without sharing any raw allocation.
pub trait Unshared {
    /// A copy sharing no raw allocation with `self`.
    #[must_use]
    fn unshared(&self) -> Self;
}

/// Values whose copy owns no allocation.
macro_rules! copy_unshared {
    ($($t:ty),* $(,)?) => {
        $(impl Unshared for $t {
            #[inline]
            fn unshared(&self) -> Self {
                *self
            }
        })*
    };
}

copy_unshared!(
    (),
    bool,
    char,
    u8,
    u16,
    u32,
    u64,
    u128,
    usize,
    i8,
    i16,
    i32,
    i64,
    i128,
    isize,
    f32,
    f64,
    std::num::NonZeroU32,
    std::num::NonZeroU64,
    std::num::NonZeroUsize,
    verter_span::Span,
);

impl<T: Copy, const N: usize> Unshared for [T; N] {
    fn unshared(&self) -> Self {
        *self
    }
}

impl Unshared for String {
    fn unshared(&self) -> Self {
        self.clone()
    }
}

impl Unshared for Box<str> {
    fn unshared(&self) -> Self {
        self.clone()
    }
}

impl<T: Unshared> Unshared for Box<T> {
    fn unshared(&self) -> Self {
        Box::new((**self).unshared())
    }
}

impl<T: Unshared> Unshared for Box<[T]> {
    fn unshared(&self) -> Self {
        self.iter().map(Unshared::unshared).collect()
    }
}

impl<T: Unshared> Unshared for Vec<T> {
    fn unshared(&self) -> Self {
        self.iter().map(Unshared::unshared).collect()
    }
}

impl<T: Unshared> Unshared for Option<T> {
    fn unshared(&self) -> Self {
        self.as_ref().map(Unshared::unshared)
    }
}

impl<A: Unshared, B: Unshared> Unshared for (A, B) {
    fn unshared(&self) -> Self {
        (self.0.unshared(), self.1.unshared())
    }
}

impl<A: Unshared, B: Unshared, C: Unshared> Unshared for (A, B, C) {
    fn unshared(&self) -> Self {
        (self.0.unshared(), self.1.unshared(), self.2.unshared())
    }
}

impl Unshared for Arc<str> {
    /// A fresh allocation of the same text.
    fn unshared(&self) -> Self {
        Arc::from(&**self)
    }
}

impl<T: Unshared> Unshared for Arc<T> {
    /// A fresh allocation of an unshared copy of the value.
    fn unshared(&self) -> Self {
        Arc::new((**self).unshared())
    }
}

impl<T: Unshared> Unshared for Arc<[T]> {
    /// A fresh allocation of unshared copies of the items.
    fn unshared(&self) -> Self {
        self.iter().map(Unshared::unshared).collect()
    }
}

impl<K, V, S> Unshared for std::collections::HashMap<K, V, S>
where
    K: Unshared + Eq + std::hash::Hash,
    V: Unshared,
    S: std::hash::BuildHasher + Clone,
{
    fn unshared(&self) -> Self {
        let mut copy = Self::with_capacity_and_hasher(self.len(), self.hasher().clone());
        copy.extend(
            self.iter()
                .map(|(key, value)| (key.unshared(), value.unshared())),
        );
        copy
    }
}

impl Unshared for OwnedStr {
    /// Another owning handle: the text's charge lives in its allocation and
    /// follows every handle.
    fn unshared(&self) -> Self {
        self.clone()
    }
}

impl<T: Measured> Unshared for OwnedSlice<T> {
    /// Another owning handle (see [`OwnedStr`]'s).
    fn unshared(&self) -> Self {
        self.clone()
    }
}

impl<T: Measured> Unshared for Owned<T> {
    /// Another owning handle (see [`OwnedStr`]'s).
    fn unshared(&self) -> Self {
        self.clone()
    }
}

impl<T: HeapSize + Send + Sync + 'static> Unshared for OwnedValue<T> {
    /// Another owning handle (see [`OwnedStr`]'s).
    fn unshared(&self) -> Self {
        self.clone()
    }
}

#[cfg(test)]
#[path = "unshared_tests.rs"]
mod tests;
