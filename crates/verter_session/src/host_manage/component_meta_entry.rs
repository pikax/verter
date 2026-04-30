//! `host_manage::component_meta_entry` — public component-meta query
//! entry points + audit-record dispatch.
//!
//! Phase 11c sub-plan §11c.2 Domain H. Holds the `evaluate_types`,
//! `get_component_meta`, and `get_component_meta_with_resolution`
//! public entry points along with the
//! [`ComponentMetaResultDb`](crate::component_meta_result_db::ComponentMetaResultDb)
//! cache hit / publish / dep-signature helpers and the audit-record
//! intake. Public surface remains rooted at `crate::host_manage::*`;
//! this file contributes a continuation `impl VerterHost { … }` block.

use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use crate::types::*;
use crate::VerterHost;

use super::{
    component_meta_debug, component_meta_debug_enabled, component_meta_options_fingerprint,
    extract_component_meta_from_resolved, ComponentMetaOptions, HostFenceValidator,
};

impl VerterHost {
    pub fn evaluate_types(
        &self,
        canonical_or_alias: &str,
    ) -> Option<verter_semantic::analysis::type_expand::ExpandedComponentTypes> {
        self.provenance
            .evaluate_types_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let resolved = self
            .resolve_component_meta(canonical_or_alias, crate::types::ProjectionMode::Expanded)?;
        resolved.evaluated_types
    }

    /// Single native component-meta query.
    ///
    /// Uses `resolve_component_meta(Expanded)` as the single enrichment owner,
    /// then projects the result through the analysis-owned `extract_component_meta`.
    ///
    /// Phase 3 wires this through
    /// [`ComponentMetaResultDb`](crate::component_meta_result_db::ComponentMetaResultDb):
    /// the method consults the project-global result cache first, revalidates
    /// the cached entry's dep-signature against the live host, and only falls
    /// back to the cold resolver path on miss or stale signature. The cold
    /// build runs inside a [`CompletionFence`](crate::completion_fence::CompletionFence)
    /// bounded to 3 attempts; repeated revalidation failures surface as a
    /// top-level `None` result rather than a publish of torn state.
    ///
    /// Returns `None` if the file doesn't exist.
    pub fn get_component_meta(
        &self,
        canonical_or_alias: &str,
    ) -> Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis> {
        self.provenance
            .get_component_meta_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let started = component_meta_debug_enabled().then(Instant::now);
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        // Phase 3: try the final-result cache before installing a request
        // view. A warm hit with a valid dep-signature returns with zero
        // resolver work.
        if let Some(warm) = self.try_component_meta_cache_hit(canonical.as_str()) {
            if let Some(started) = started {
                component_meta_debug(format!(
                    "get_component_meta owner={} warm-cache hit took {:?}",
                    canonical,
                    started.elapsed(),
                ));
            }
            return Some(warm);
        }

        // Cold build — install a request view for the duration of the
        // existing resolver path. Phase 4 replaces the view with fence
        // observation only.
        let resolved = self
            .resolve_component_meta(canonical.as_str(), crate::types::ProjectionMode::Expanded)?;
        // Always include fallthrough — the solver path does not use walker
        // overflow as a gating signal.
        let meta = extract_component_meta_from_resolved(
            self,
            canonical.as_str(),
            &resolved,
            true, // include_fallthrough
        );

        self.publish_component_meta_cache_entry(canonical.as_str(), &resolved, meta.clone());

        if let Some(started) = started {
            component_meta_debug(format!(
                "get_component_meta owner={} cold took {:?}",
                canonical,
                started.elapsed(),
            ));
        }
        Some(meta)
    }

    /// Phase 3: look up the project-global final-result cache for the
    /// owner and return the warm payload only when its recorded
    /// dep-signature revalidates against the live host. Returns `None` on
    /// any miss, stale entry, or missing shallow state.
    fn try_component_meta_cache_hit(
        &self,
        canonical: &str,
    ) -> Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis> {
        let shallow = self.shallow_file_state(canonical)?;
        let key = crate::component_meta_result_db::ComponentMetaResultKey {
            owner_canonical: Arc::from(canonical),
            owner_whole_hash: shallow.whole_hash,
            query_kind: crate::component_meta_result_db::ComponentMetaQueryKind::Native,
            options_fingerprint: component_meta_options_fingerprint(
                &ComponentMetaOptions::default(),
            ),
        };
        let entry = self.project_type_store.component_meta_results().get(&key)?;
        let validator = HostFenceValidator { host: self };
        use crate::completion_fence::FenceValidator;
        let dep_sig_valid = entry
            .dep_signature
            .iter()
            .all(|(canonical_id, version)| validator.validate(canonical_id, version));
        if !dep_sig_valid {
            return None;
        }
        // Step 4 (architectural-debt-closure rev 10): the DB now stores
        // `CachedComponentMetaResult { analysis, resolution_template, ... }`
        // so the with_resolution path can rehydrate without re-running the
        // cold resolver. The plain `get_component_meta` warm path returns
        // only the analysis projection.
        Some(entry.payload.analysis.clone())
    }

    /// Phase 3: publish the cold-build result into the project-global
    /// final-result cache. The dep-signature carries the owner's whole-hash,
    /// the current project generation, and every transitive file fact the
    /// resolver observed while producing the result. A later lookup
    /// revalidates the full signature against the live host so an edit to
    /// *any* file the resolver touched invalidates the cached payload — not
    /// just edits to the owner itself.
    fn publish_component_meta_cache_entry(
        &self,
        canonical: &str,
        resolved: &crate::meta_resolve::ResolvedComponentMetaState,
        meta: verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
    ) {
        let Some(shallow) = self.shallow_file_state(canonical) else {
            return;
        };
        let whole_hash = shallow.whole_hash;
        let key = crate::component_meta_result_db::ComponentMetaResultKey {
            owner_canonical: Arc::from(canonical),
            owner_whole_hash: whole_hash,
            query_kind: crate::component_meta_result_db::ComponentMetaQueryKind::Native,
            options_fingerprint: component_meta_options_fingerprint(
                &ComponentMetaOptions::default(),
            ),
        };
        let dep_signature = Self::build_component_meta_dep_signature(
            canonical,
            whole_hash,
            self.project_type_store.project_generation(),
            &resolved.fact_versions,
        );
        let resolution_template =
            crate::component_meta_result_db::ResolutionTemplate::from_resolved_state(resolved);
        let cached = crate::component_meta_result_db::CachedComponentMetaResult {
            analysis: meta,
            resolution_template,
            canonical_id: Arc::from(canonical),
            whole_hash,
        };
        self.project_type_store.component_meta_results().insert(
            key,
            crate::component_meta_result_db::ComponentMetaResultEntry {
                payload: Arc::new(cached),
                dep_signature,
            },
        );
    }

    /// Lower the resolver's observed fact-version list into a transitive
    /// `DepSignature`. Owner + project-generation facts always participate;
    /// file whole-hashes discovered during resolution are deduped per
    /// canonical so a single entry per touched file ends up in the signature.
    /// Derived-fact hashes (route / import-route) are intentionally skipped
    /// for now — they are validated via their underlying file hashes plus
    /// the project-generation bump on shape changes. Including them in the
    /// signature would require extending `HostFenceValidator` with a
    /// derived-fact-aware path, which lands with the Phase 4 cut.
    fn build_component_meta_dep_signature(
        owner_canonical: &str,
        owner_whole_hash: Hash16,
        project_gen: u64,
        fact_versions: &[crate::resolver_core::FactVersionRef],
    ) -> crate::semantic_query::DepSignature {
        use crate::semantic_query::DepVersion;
        let mut entries: Vec<(Arc<str>, DepVersion)> = Vec::with_capacity(fact_versions.len() + 2);
        entries.push((
            Arc::<str>::from(owner_canonical),
            DepVersion::WholeHash(owner_whole_hash),
        ));
        entries.push((
            Arc::<str>::from(owner_canonical),
            DepVersion::ProjectGeneration(project_gen),
        ));
        let mut seen: rustc_hash::FxHashSet<(Arc<str>, Hash16)> = rustc_hash::FxHashSet::default();
        seen.insert((Arc::<str>::from(owner_canonical), owner_whole_hash));
        for fact in fact_versions {
            if let crate::resolver_core::FactVersionRef::FileWholeHash { canonical_id, hash } = fact
            {
                let canonical: Arc<str> = Arc::from(canonical_id.as_str());
                if seen.insert((canonical.clone(), *hash)) {
                    entries.push((canonical, DepVersion::WholeHash(*hash)));
                }
            }
        }
        Arc::from(entries.into_boxed_slice())
    }

    /// Combined query: resolves component-meta once and returns both the
    /// analysis projection and the resolved-meta sidecar. Avoids the
    /// double `resolve_component_meta(Expanded)` that happens if callers
    /// invoke `get_component_meta()` + `resolve_component_meta()` separately.
    ///
    /// **Plan §3 Step 4 (architectural-debt-closure rev 10).** Consults
    /// the `ComponentMetaResultDb` warm cache before falling through to
    /// the cold resolver. On a cache hit with a valid `dep_signature`,
    /// the cached `ResolutionTemplate` rehydrates a per-request
    /// `ResolvedComponentMetaState` (snapshot reloaded from `IndexedReadyDb`)
    /// and a synthesized `RustAuditRecord` with `from_cache = true`,
    /// `total_ms = 0.0` is published into `host.audit_records` so audit
    /// consumers via `take_audit_record(resolution.request_id)` work
    /// uniformly.
    pub fn get_component_meta_with_resolution(
        &self,
        canonical_or_alias: &str,
    ) -> Option<(
        verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
        crate::meta_resolve::ResolvedComponentMetaState,
    )> {
        self.provenance
            .get_component_meta_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Stamp a request id for this call. The `AuditedRequest`
        // harness (Commit 6) tracks this via `REQUESTS_CREATED_IN_CURRENT_AUDITED_RUN`
        // so multi-request closures inside `run_custom` can be rejected.
        let request_id = self.next_request_id();
        crate::request_context::increment_requests_created();

        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);

        // Install a `RequestContext` when footprint capture is enabled.
        // The guard restores the prior TLS state on drop (normal return
        // AND panic unwind). If capture is disabled, we still create a
        // lightweight context so `current_request_id()` works for
        // attribution — but no accumulator is attached.
        let footprint_capture = self.config.footprint_capture && self.config.audit_enabled;
        let accumulator = if footprint_capture {
            Some(std::sync::Arc::new(
                crate::component_meta_audit::RequestFootprintAccumulator::new(),
            ))
        } else {
            None
        };
        let ctx = crate::request_context::RequestContext::new(
            request_id,
            std::sync::Arc::<str>::from(canonical.as_str()),
            footprint_capture,
            accumulator.clone(),
        );

        // Register a per-request `SessionVfsSink` with the workspace
        // so VFS reads populate the accumulator's `vfs_reads`. The
        // registration must outlive the `RequestContextGuard` below
        // so late events still route correctly; it is dropped FIRST
        // at scope exit (field order: `_sink_registration` above
        // `_ctx_guard` would drop registration LAST, which we want).
        //
        // Rust drops locals in REVERSE declaration order, so we
        // declare the guard FIRST and the registration SECOND: at
        // scope exit, the registration drops first (deregistering
        // the sink — no more fan-out events arrive), then the
        // context guard drops, then the accumulator Arc drops.
        //
        // Plan §3.A Commit 6.D.
        let _ctx_guard = crate::request_context::RequestContextGuard::install(ctx);
        let _sink_registration = accumulator.as_ref().and_then(|acc| {
            let sink = crate::component_meta_audit::session_vfs_sink::SessionVfsSink::new(
                request_id,
                std::sync::Arc::clone(acc),
            );
            self.workspace().register_audit_sink(sink).ok()
        });

        // Plan §3 Step 4: warm-cache short-circuit AFTER request-context
        // install (so `current_request_id()` returns the fresh id even
        // on the warm path). Validates `dep_signature` against current
        // host state; on success, rehydrates the resolution template
        // and synthesizes a `from_cache: true` audit record.
        if let Some((analysis, resolution)) =
            self.try_with_resolution_cache_hit(canonical.as_str(), request_id)
        {
            return Some((analysis, resolution));
        }

        let mut resolved = self
            .resolve_component_meta(canonical.as_str(), crate::types::ProjectionMode::Expanded)?;
        resolved.request_id = request_id;
        // Always include fallthrough — the solver path does not use walker
        // overflow as a gating signal.
        let analysis = extract_component_meta_from_resolved(
            self,
            canonical.as_str(),
            &resolved,
            true, // include_fallthrough
        );

        // Plan §3 Step 4: cache-write so subsequent identical calls
        // short-circuit through `try_with_resolution_cache_hit`.
        self.publish_component_meta_cache_entry(canonical.as_str(), &resolved, analysis.clone());

        Some((analysis, resolved))
    }

    /// Plan §3 Step 4 cache-hit path (architectural-debt-closure rev 10).
    /// Returns `Some((analysis, resolution))` on a valid warm hit; `None`
    /// otherwise (miss, stale `dep_signature`, or eviction-race rehydrate
    /// failure). Caller falls through to the cold resolver on `None`.
    ///
    /// Synthesizes a `RustAuditRecord` with `from_cache = true` and
    /// `total_ms = 0.0` and publishes it into `host.audit_records` (when
    /// audit is on) so `take_audit_record(resolution.request_id)`
    /// returns it uniformly with cold-resolver records.
    fn try_with_resolution_cache_hit(
        &self,
        canonical: &str,
        request_id: u64,
    ) -> Option<(
        verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
        crate::meta_resolve::ResolvedComponentMetaState,
    )> {
        let shallow = self.shallow_file_state(canonical)?;
        let key = crate::component_meta_result_db::ComponentMetaResultKey {
            owner_canonical: Arc::from(canonical),
            owner_whole_hash: shallow.whole_hash,
            query_kind: crate::component_meta_result_db::ComponentMetaQueryKind::Native,
            options_fingerprint: component_meta_options_fingerprint(
                &ComponentMetaOptions::default(),
            ),
        };
        let entry = self.project_type_store.component_meta_results().get(&key)?;
        let validator = HostFenceValidator { host: self };
        use crate::completion_fence::FenceValidator;
        let dep_sig_valid = entry
            .dep_signature
            .iter()
            .all(|(canonical_id, version)| validator.validate(canonical_id, version));
        if !dep_sig_valid {
            return None;
        }

        // Rehydrate the resolution template into a fresh per-request state.
        // Returns None on the bounded eviction race where the snapshot
        // was evicted between dep_signature validation and reload.
        let cached = entry.payload;
        let resolution = cached.resolution_template.rehydrate(
            self,
            &cached.canonical_id,
            cached.whole_hash,
            request_id,
        )?;

        // Synthesize a from_cache audit record so consumers via
        // `take_audit_record(resolution.request_id)` get uniform
        // observability.
        if self.config.audit_enabled {
            let synthesized = crate::component_meta_audit::RustAuditRecord {
                request_id,
                canonical_id: canonical.to_string(),
                timings: crate::component_meta_audit::RustTimingAudit::default(),
                solver: crate::component_meta_audit::RustSolverAudit::default(),
                store: crate::component_meta_audit::RustStoreAudit::default(),
                memory: crate::component_meta_audit::RustMemoryAudit::default(),
                footprint: None,
                from_cache: true,
            };
            debug_assert_eq!(synthesized.request_id, resolution.request_id);
            self.publish_audit_record(synthesized);
        }

        Some((cached.analysis.clone(), resolution))
    }

    /// Monotonic request-id generator. Starts at 1; zero is reserved
    /// for "not populated" (see `ResolvedComponentMetaState::request_id`).
    pub(crate) fn next_request_id(&self) -> u64 {
        use std::sync::atomic::Ordering;
        self.request_id_counter.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Drain the `RustAuditRecord` matching `request_id` from the host's
    /// bounded audit-record store. Returns `None` when the record was
    /// never inserted (capture disabled) or already drained by a prior
    /// `take_audit_record` call. Plan §1.4 / §2.5.
    pub fn take_audit_record(
        &self,
        request_id: u64,
    ) -> Option<crate::component_meta_audit::RustAuditRecord> {
        self.audit_records.take(request_id)
    }

    /// Publish a finished audit record into the host's store. Typically
    /// called by `emit_audit_trace` once per audited request.
    pub fn publish_audit_record(&self, record: crate::component_meta_audit::RustAuditRecord) {
        self.audit_records.insert(record);
    }
}
