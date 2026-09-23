//! Guarded access to the session host: the handle a concurrent consumer holds.
//!
//! The session releases a closed document's semantic nodes only while no
//! computation is in flight ([`crate::project_type_store::semantic_activity`]):
//! a computation that read one of those nodes must never see it turn into the
//! released placeholder between two of its reads. A consumer that reaches the
//! host from more than one thread at a time and closes documents must
//! therefore run every host call under a [`SemanticActivityGuard`]; this is
//! the session's own handle for doing so, so that the protocol is the crate's
//! to offer rather than each consumer's to remember.
//!
//! [`GuardedHost`] deliberately does not dereference to the host: its only
//! accessor, [`GuardedHost::host`], returns a [`HostRef`] that owns a guard. A
//! `&VerterHost` borrowed from a `HostRef` cannot outlive it, so a host call
//! can only run while its guard is alive: completeness is enforced by the
//! borrow checker rather than by convention. Node reads happen only inside
//! host calls (the host returns owned results, never node ids), so one guard
//! per call is exactly the required granularity.
//!
//! Which consumers need it is pinned by the crate's
//! `semantic_activity_boundary` test: the language server (many threads, and
//! the one consumer that closes documents) holds the host only through this
//! handle; the NAPI and WASM bindings are synchronous on one thread, so a
//! close they issue runs to completion before any other call; the MCP server,
//! the CLI and the baseline runner never close a document, so their gate never
//! has anything queued. The test fails when a binding grows a thread or a
//! never-closing consumer grows a close, which is the moment it must adopt
//! this handle.

use std::sync::Arc;

use crate::project_type_store::SemanticActivityGuard;
use crate::VerterHost;

/// An owned, cloneable handle to the session host. It deliberately does not
/// dereference to the host: every access goes through [`Self::host`].
#[derive(Clone)]
pub struct GuardedHost(Arc<VerterHost>);

impl std::fmt::Debug for GuardedHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GuardedHost").finish_non_exhaustive()
    }
}

impl GuardedHost {
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
