//! Scope guards and the innermost-open-site thread-local.
//!
//! COMPILED ONLY under the `attribution` feature.
//!
//! A scope guard does two things: it times its region (inclusive of
//! nested guards, so a caller's number contains its callees'), and it
//! publishes its site as the thread's innermost open scope so
//! [`super::alloc`] can attribute heap traffic to it.
//!
//! The thread-local is a `const`-initialised [`Cell`] on purpose: a
//! `const`-init TLS has no lazy initialiser and no destructor, so
//! reading it never allocates. That is a hard requirement, because the
//! global allocator reads it on every allocation and an allocating read
//! would recurse forever.

use std::cell::Cell;
use std::marker::PhantomData;

use super::schema::WorkSite;
use super::table::record_scope;
use crate::instant::Instant;

/// Sentinel meaning "no scope is open on this thread".
pub(super) const NO_SITE: u32 = u32::MAX;

thread_local! {
    static CURRENT_SITE: Cell<u32> = const { Cell::new(NO_SITE) };
}

/// The innermost open scope on this thread, if any.
#[inline]
pub(super) fn current_site_index() -> Option<usize> {
    CURRENT_SITE
        .try_with(|slot| slot.get())
        .ok()
        .filter(|raw| *raw != NO_SITE)
        .map(|raw| raw as usize)
}

/// Times a region and owns it for allocation attribution.
///
/// Created by [`crate::attribute_scope!`]. Nesting is innermost-wins for
/// allocation and inclusive for time.
///
/// The guard is `!Send`: it saves the PREVIOUS site of the thread that
/// opened it and restores that value on drop, so dropping it on another
/// thread would write one thread's scope stack into another's and
/// mis-attribute every allocation that follows. The raw-pointer
/// `PhantomData` makes that a compile error rather than a silent
/// measurement bug for anyone extending the substrate.
pub struct ScopeGuard {
    site: WorkSite,
    previous: u32,
    start: Instant,
    /// Binds the guard to its opening thread — see the type docs.
    _not_send: PhantomData<*const ()>,
}

impl ScopeGuard {
    /// Open a scope for `site`.
    #[inline]
    pub fn enter(site: WorkSite) -> Self {
        let previous = CURRENT_SITE
            .try_with(|slot| slot.replace(site.index() as u32))
            .unwrap_or(NO_SITE);
        Self {
            site,
            previous,
            start: Instant::now(),
            _not_send: PhantomData,
        }
    }

    /// The site this guard is timing.
    #[inline]
    pub fn site(&self) -> WorkSite {
        self.site
    }
}

impl Drop for ScopeGuard {
    #[inline]
    fn drop(&mut self) {
        let elapsed = self.start.elapsed().as_nanos();
        // A 585-year region would be needed to saturate this; the clamp
        // exists so a pathological clock reading cannot wrap the column.
        record_scope(self.site, elapsed.min(u64::MAX as u128) as u64);
        let previous = self.previous;
        let _ = CURRENT_SITE.try_with(|slot| slot.set(previous));
    }
}
