//! Guarded access to the session host.
//!
//! The session releases a closed document's semantic nodes only while no
//! computation is in flight (`verter_session::project_type_store::semantic_activity`): a
//! computation that read one of those nodes must never see it turn into the
//! released placeholder between two of its reads. Every host call the language
//! server makes — from a request handler, a background loop or a blocking
//! task — therefore runs under a `SemanticActivityGuard`.
//!
//! [`SharedHost`] is the ONLY handle this crate keeps to a host it closes
//! documents on, and its only accessor, [`SharedHost::host`], returns a
//! [`HostRef`] that owns a guard. The handle is the session's own
//! ([`verter_session::project_type_store::GuardedHost`]): the protocol is the crate that releases
//! nodes offering it, and `verter_session`'s `semantic_activity_boundary` test
//! pins that this crate stores no raw `Arc<VerterHost>` outside the files that
//! build the handle or read project identity only.

pub use verter_session::project_type_store::guarded_host::{GuardedHost as SharedHost, HostRef};
