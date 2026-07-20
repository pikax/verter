//! Typeinfo request-footprint attachment.
//!
//! The audited typeinfo entry-points ([`crate::VerterHost::resolve_named_symbol_with_audit`],
//! [`crate::VerterHost::evaluate_type_expression_with_audit`],
//! [`crate::VerterHost::resolve_type_with_audit`]) attach a mined
//! [`verter_audit::RequestFootprintAudit`] to their records through the SAME
//! passive-observer pipeline the component-meta entry uses
//! (`install_component_meta_audit_scope` → accumulator → `SessionVfsSink` →
//! drain → `build_file_audit_vec` → `mine_footprint`): one per-request
//! [`crate::component_meta_audit::RequestFootprintAccumulator`] is planted on
//! the [`crate::request_context::RequestContext`], the workspace VFS audit
//! sink attributes this request's reads to it, and after the request body
//! completes the drained state is mined into the footprint + per-file
//! attribution vector.
//!
//! Contract (the footprint-attachment-on-typeinfo contract): every audited
//! typeinfo request attaches a footprint when `footprint_capture=true` on
//! [`crate::types::HostConfig`] — warm or cold, hit or miss — so edit-cycle
//! contracts (`typeinfo_tests::cache_invalidation`) can characterise which
//! files a re-resolve actually touched. When capture is off the record keeps
//! `footprint: None`, matching the component-meta contract.

use std::sync::Arc;

use crate::request_context::RequestContext;
use crate::VerterHost;

/// RAII bundle for one audited typeinfo request's footprint capture: the
/// per-request accumulator (also planted on the [`RequestContext`]) plus the
/// workspace VFS audit-sink registration attributing this request's reads.
/// The sink holds a `Weak` to the accumulator, so late fan-out events no-op
/// once this scope (and the context's accumulator `Arc`) drops.
pub(crate) struct TypeinfoFootprintScope {
    accumulator: Option<Arc<crate::component_meta_audit::RequestFootprintAccumulator>>,
    _sink_handle: Option<verter_workspace::audit_sink::SinkHandle>,
}

impl TypeinfoFootprintScope {
    /// Build the per-request accumulator (when `footprint_capture` is on)
    /// and register the per-request `SessionVfsSink` on the workspace so
    /// VFS reads performed by this request attribute to it. Mirrors the
    /// component-meta scope installer.
    pub(crate) fn install(host: &VerterHost, request_id: u64, footprint_capture: bool) -> Self {
        let accumulator = if footprint_capture {
            Some(Arc::new(
                crate::component_meta_audit::RequestFootprintAccumulator::with_caps(
                    host.config.audit_caps.clone(),
                ),
            ))
        } else {
            None
        };
        let sink_handle = accumulator.as_ref().and_then(|acc| {
            let sink = crate::component_meta_audit::session_vfs_sink::SessionVfsSink::new(
                request_id,
                Arc::clone(acc),
            );
            host.workspace().register_audit_sink(sink).ok()
        });
        Self {
            accumulator,
            _sink_handle: sink_handle,
        }
    }

    /// The accumulator to plant on the request's [`RequestContext`]
    /// (`None` when capture is off).
    pub(crate) fn accumulator(
        &self,
    ) -> Option<Arc<crate::component_meta_audit::RequestFootprintAccumulator>> {
        self.accumulator.clone()
    }
}

/// Drain the request's accumulator and mine the deterministic footprint +
/// per-file attribution vector — the SAME finalisation path the
/// component-meta cold resolver and warm-replay branches use
/// (`build_file_audit_vec` + `mine_footprint`). Returns `(None, [])` when
/// capture is off or no accumulator is attached, so the caller's record
/// keeps `footprint: None` exactly as before.
pub(crate) fn mine_typeinfo_footprint(
    host: &VerterHost,
    ctx: &RequestContext,
) -> (
    Option<verter_audit::RequestFootprintAudit>,
    Vec<verter_audit::files::FileAudit>,
) {
    if !ctx.footprint_capture {
        return (None, Vec::new());
    }
    let Some(acc) = ctx.audit_accumulator.as_ref() else {
        return (None, Vec::new());
    };
    let state = acc.drain();
    // Direct imports of the entry file: lets the file-role classifier
    // distinguish first-level imports (`DirectImport`) from deeper-closure
    // files (`TransitiveImport`). An absent shallow surface (rare cold
    // path) falls back to `DirectImport` for every non-Entry file.
    let direct_imports: rustc_hash::FxHashSet<String> = host
        .shallow_file_state(ctx.canonical_id.as_ref())
        .map(|sfs| {
            sfs.import_targets
                .values()
                .map(|t| t.canonical_id.clone())
                .collect()
        })
        .unwrap_or_default();
    let files = crate::component_meta_audit::build_file_audit_vec(
        &state,
        ctx.canonical_id.as_ref(),
        &direct_imports,
        host.config.audit_timing_capture && host.config.audit_enabled,
    );
    let footprint = crate::component_meta_audit::mine_footprint(
        host.project_type_store().semantic_graph(),
        state,
        ctx,
        host.config.max_derivation_edges,
        &host.config.audit_caps,
    );
    (Some(footprint), files)
}
