//! Guarded access to the session host.
//!
//! The session releases a closed document's semantic nodes only while no
//! computation is in flight (`verter_session::project_type_store::semantic_activity`): a
//! computation that read one of those nodes must never see it turn into the
//! released placeholder between two of its reads. Every host call the language
//! server makes — from a request handler, a background loop or a blocking
//! task — therefore runs under a `SemanticActivityGuard`.
//!
//! [`SharedHost`] is the ONLY handle this crate keeps to the host, and its only
//! accessor, [`SharedHost::host`], returns a [`HostRef`] that owns a guard. A
//! `&VerterHost` borrowed from a `HostRef` cannot outlive it, so a host call can
//! only run while its guard is alive: completeness is enforced by the borrow
//! checker rather than by convention. Node reads happen only inside host calls
//! (the host returns owned results, never node ids), so one guard per call is
//! exactly the required granularity.

use std::sync::Arc;

use verter_session::project_type_store::SemanticActivityGuard;
use verter_session::VerterHost;

/// An owned, cloneable handle to the session host. It deliberately does not
/// dereference to the host: every access goes through [`Self::host`].
#[derive(Clone)]
pub struct SharedHost(Arc<VerterHost>);

impl std::fmt::Debug for SharedHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedHost").finish_non_exhaustive()
    }
}

impl SharedHost {
    /// Wrap the session host.
    #[must_use]
    pub fn new(host: Arc<VerterHost>) -> Self {
        Self(host)
    }

    /// Borrow the host for one call (or one short sequence of calls) under a
    /// semantic-activity guard.
    #[must_use]
    pub fn host(&self) -> HostRef<'_> {
        HostRef {
            _activity: self.0.semantic_activity(),
            host: &self.0,
        }
    }

    /// Run `f` with the host `Arc` (for host APIs whose receiver is
    /// `&Arc<VerterHost>`) under a semantic-activity guard.
    pub fn with_arc<R>(&self, f: impl FnOnce(&Arc<VerterHost>) -> R) -> R {
        let _activity = self.0.semantic_activity();
        f(&self.0)
    }
}

/// A host reference that holds a semantic-activity guard for as long as it
/// lives. Dereferences to [`VerterHost`].
pub struct HostRef<'a> {
    _activity: SemanticActivityGuard,
    host: &'a VerterHost,
}

impl std::ops::Deref for HostRef<'_> {
    type Target = VerterHost;

    fn deref(&self) -> &VerterHost {
        self.host
    }
}
