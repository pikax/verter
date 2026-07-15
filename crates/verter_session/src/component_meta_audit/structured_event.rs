#![deny(missing_docs)]
//! In-crate re-export of [`verter_audit::structured_event`].
//!
//! `StructuredAuditEvent` (the renamed authoritative enum) and its
//! variant payload types live in `verter_audit::origin_graph`. This
//! module preserves the historic
//! `verter_session::component_meta_audit::structured_event::*` import
//! path so the session's own modules
//! (`host_manage::component_meta_trace_structured!` and friends) do
//! not need to retarget every import to the substrate.

pub use verter_audit::structured_event::StructuredAuditEvent;
