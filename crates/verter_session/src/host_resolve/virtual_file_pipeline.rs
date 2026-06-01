//! `impl VerterHost` — the SFC virtual-file pipeline.
//!
//! Owns the public `resolve` / `ensure_compiled` / `compile_slot_is_warm`
//! / `get_virtual_file` / `list_virtual_files` / `get_ide` /
//! `get_public_api*` accessors, the `store_latest_diagnostics` writer,
//! and the internal `compile_entry` / `hydrate_compile_blockers` helpers
//! that drive on-demand SFC compilation through the scheduler-backed
//! cache substrate.

use std::sync::Arc;

use rustc_hash::FxHashMap;

#[cfg(feature = "session_metrics")]
use crate::instant::Instant;

use super::vue_script_extract::template_converter_inputs;
use crate::compile::{assemble_main_module, merge_external_sources};
use crate::hash::compile_profile_hash;
use crate::id::{parse_raw_id, render_ids, render_single_id};
use crate::types::*;
use crate::VerterHost;
use oxc_allocator::Allocator;
use verter_compiler::compile::CodegenOptions;
use verter_compiler::compile::{
    compile as compile_sfc, compile_from_parsed, format_import_specifier, VerterCompileOptions,
};

/// Test-only fact-injection knob for the compile-tier cold-build
/// path. When set to `N > 0`, the cold-compute closure observes `N`
/// synthetic `FileWholeHash` facts via `observe_fan_out` after the
/// normal compile-tier observation step, deterministically forcing
/// the installed fact tracer to either overflow (when `N >
/// FACT_SIGNATURE_CAP`) or accumulate a large signature. Drives
/// discriminating tests of the refuse-publish-on-overflow contract
/// without requiring a pathological workspace fixture that organically
/// produces > 1024 facts.
///
/// The flag is reset to 0 by the RAII guard
/// [`CompileForceOverflowGuard`] after the test completes so
/// concurrent tests are not affected. Production reads it once per
/// cold compute as a relaxed atomic load (~1 ns); the load path lives
/// on the cold-build path which already takes locks, so the cost is
/// in the noise.
#[doc(hidden)]
pub(crate) static COMPILE_TEST_FORCE_OVERFLOW_OBSERVATIONS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// RAII guard that clears [`COMPILE_TEST_FORCE_OVERFLOW_OBSERVATIONS`]
/// on drop. Test setup arms the desired observation count; the guard
/// drops at scope exit and restores the baseline so a panic / early
/// return does not leak the forced state into concurrent tests.
#[doc(hidden)]
#[cfg(any(test, debug_assertions))]
pub struct CompileForceOverflowGuard;

#[cfg(any(test, debug_assertions))]
impl CompileForceOverflowGuard {
    /// Set the forced observation count to `n` and return the guard.
    pub(crate) fn arm(n: usize) -> Self {
        COMPILE_TEST_FORCE_OVERFLOW_OBSERVATIONS.store(n, std::sync::atomic::Ordering::Relaxed);
        Self
    }
}

#[cfg(any(test, debug_assertions))]
impl Drop for CompileForceOverflowGuard {
    fn drop(&mut self) {
        COMPILE_TEST_FORCE_OVERFLOW_OBSERVATIONS.store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Test/debug-only invocation counter for
/// [`VerterHost::prefetch_compile_tier_observation_targets`]. Incremented
/// once per actual call to the prefetch. The cold-compute path installs
/// the prefetch ONLY for the `Session` cache mode (it pre-populates the
/// compile-tier fact tracer, which is itself installed only for
/// `Session`); `Content` / `Stateless` compile with no fact rail and
/// therefore never invoke it. This counter lets a routing test assert
/// exactly that gate — it stays `0` across a `Content` / `Stateless`
/// cold compute and increments on a `Session` cold compute.
///
/// Gated to `cfg(any(test, debug_assertions))` so release builds carry
/// neither the atomic nor the increment.
#[doc(hidden)]
#[cfg(any(test, debug_assertions))]
pub(crate) static COMPILE_TIER_PREFETCH_INVOCATIONS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Reset [`COMPILE_TIER_PREFETCH_INVOCATIONS`] to zero. Test setup calls
/// this immediately before a cold compute so the post-compute read
/// reflects only that compute's prefetch invocations, independent of any
/// earlier compile in the same process.
#[doc(hidden)]
#[cfg(any(test, debug_assertions))]
pub fn reset_compile_tier_prefetch_invocations() {
    COMPILE_TIER_PREFETCH_INVOCATIONS.store(0, std::sync::atomic::Ordering::Relaxed);
}

/// Current value of [`COMPILE_TIER_PREFETCH_INVOCATIONS`]. Reads the
/// relaxed atomic; pair with [`reset_compile_tier_prefetch_invocations`]
/// around a single cold compute for a deterministic observation.
#[doc(hidden)]
#[cfg(any(test, debug_assertions))]
pub fn compile_tier_prefetch_invocations() -> usize {
    COMPILE_TIER_PREFETCH_INVOCATIONS.load(std::sync::atomic::Ordering::Relaxed)
}

/// Compile-mode discriminator for the content-addressed cache key:
/// the `Content` cache-mode stable hash folded with the full profile
/// identity. The profile (`DefaultHasher`-based; in-memory only, never
/// persisted) captures every codegen-affecting flag so two `Content`
/// requests with different profiles do not collide on one content entry.
fn content_mode_profile_hash(profile: &CompileProfile) -> Hash16 {
    let mut buf = Vec::with_capacity(40);
    buf.extend_from_slice(b"verter.content_mode_profile.v1:");
    buf.extend_from_slice(&CompileCacheMode::Content.stable_hash());
    buf.extend_from_slice(&compile_profile_hash(profile).to_le_bytes());
    crate::hash::hash_16(&buf)
}

/// Deployment version hash for the compiler crate. Two builds of a
/// different compiler version must not share a content-addressed cache
/// entry (the codegen may differ byte-for-byte). Derived from the
/// crate semantic version; the compiler and session crates version in
/// lockstep across the workspace.
fn compiler_version_hash() -> Hash16 {
    crate::hash::hash_16(concat!("verter.compiler.v1:", env!("CARGO_PKG_VERSION")).as_bytes())
}

/// Deployment version hash for the codegen plugin set. The compile
/// pipeline is monolithic (no separately-versioned plugin registry),
/// so the plugin-set identity tracks the crate semantic version in
/// lockstep with [`compiler_version_hash`].
fn plugin_versions_hash() -> Hash16 {
    crate::hash::hash_16(concat!("verter.plugins.v1:", env!("CARGO_PKG_VERSION")).as_bytes())
}

impl VerterHost {
    /// Resolve a raw import identifier (bundler query string or LSP `._VERTER_.` format)
    /// to its canonical ID, virtual node kind, and rendered bundler/LSP IDs.
    ///
    /// Returns `None` if the raw ID cannot be parsed.
    pub fn resolve(&self, raw_id: &str) -> Option<ResolvedId> {
        #[cfg(feature = "session_metrics")]
        self.metrics
            .resolves
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let parsed = parse_raw_id(raw_id)?;
        let canonical = self.resolve_alias_or_canonical(&parsed.canonical_id);
        let (exists, bundler_id, lsp_id) = {
            {
                use crate::host_executor::HostSourceData;
                let meta = self.scheduler.try_get_source(&canonical).and_then(|s| {
                    s.downcast_data::<HostSourceData>()
                        .map(|h| h.parse.meta.clone())
                });
                match meta {
                    Some(m) => {
                        let (b, l) = render_ids(&canonical, &parsed.node_kind, &m);
                        (true, b, l)
                    }
                    None => {
                        let default_meta = FileMeta::default();
                        let (b, l) = render_ids(&canonical, &parsed.node_kind, &default_meta);
                        (false, b, l)
                    }
                }
            }
        };
        Some(ResolvedId {
            canonical_id: canonical,
            node_kind: parsed.node_kind,
            exists_in_host: exists,
            bundler_id,
            lsp_id,
        })
    }

    /// Ensure a file is compiled and cached for the given profile.
    ///
    /// Unlike [`get_virtual_file`](Self::get_virtual_file), this does not require
    /// specifying a `VirtualNodeKind`. It simply ensures the compilation cache is
    /// populated so that subsequent `get_ide()`, `get_analysis()`, or
    /// `get_virtual_file()` calls hit the cache.
    ///
    /// Returns `Ok(())` on success (cache hit or successful compilation).
    /// Returns `Err(HostError)` if the file is missing or compilation fails.
    fn hydrate_compile_blockers(&self, canonical_id: &str) {
        let Some(blockers) = self.get_compile_blockers(canonical_id) else {
            return;
        };

        let workspace = self.workspace();
        let mut blocker_ids = std::collections::BTreeSet::new();

        for request in blockers.external_source_requests {
            let resolved = workspace
                .resolve_import(
                    canonical_id,
                    &request.specifier,
                    verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind: verter_workspace::ResolveRequestKind::SfcSrcAttr,
                    },
                )
                .map(|resolution| {
                    self.cache_positive_import_route_result(
                        canonical_id,
                        &request.specifier,
                        &resolution.source_id,
                    );
                    resolution.source_id
                })
                .unwrap_or(request.resolved_canonical_id);
            if resolved != canonical_id {
                blocker_ids.insert(resolved);
            }
        }

        for dep in blockers.macro_type_deps.iter() {
            let resolved = workspace
                .resolve_import(
                    canonical_id,
                    &dep.import_source,
                    verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind: verter_workspace::ResolveRequestKind::TypeImport,
                    },
                )
                .inspect(|resolution| {
                    self.cache_positive_import_route_result(
                        canonical_id,
                        &dep.import_source,
                        &resolution.source_id,
                    );
                })
                .or_else(|| {
                    workspace
                        .resolve_import(
                            canonical_id,
                            &dep.import_source,
                            verter_workspace::ResolutionContext {
                                phase: verter_workspace::ResolvePhase::CodegenBlocker,
                                kind: verter_workspace::ResolveRequestKind::EsmImport,
                            },
                        )
                        .inspect(|resolution| {
                            self.cache_positive_import_route_result(
                                canonical_id,
                                &dep.import_source,
                                &resolution.source_id,
                            );
                        })
                })
                .map(|resolution| resolution.source_id);
            if let Some(resolved) = resolved.filter(|resolved| resolved != canonical_id) {
                blocker_ids.insert(resolved);
            }
        }

        for blocker_id in blocker_ids {
            let _ = self.ensure_loaded(&blocker_id);
        }
    }

    /// R3/R26/R28 cold-compute prefetch: resolve and load the cross-file
    /// dependency surface the compile-tier fact tracer will observe
    /// before the tracer is installed.
    ///
    /// The compile-tier tracer in `observe_compile_tier_dependencies`
    /// reads two pieces of state per macro-type-dep:
    ///
    /// 1. `derived_raw_cache().get(owner).import_routes` — the
    ///    owner's per-import resolution table; needed to translate
    ///    `dep.import_source` to a canonical id.
    /// 2. `VerterHost::current_content_pinned_artifacts(dep)` — the
    ///    dependency's content-pinned `FileArtifacts` entry; needed to
    ///    look up the `Member` / `MemberPresence` fact hashes. The
    ///    content pin (scheduler-authoritative hash, artifact-only
    ///    fallback) keeps the lookup off a stale lingering artifact.
    ///
    /// On a cold compute of the owner SFC neither of those is
    /// pre-populated, so without this prefetch the tracer silently
    /// records an empty signature and the consumer would never
    /// invalidate on a cross-file edit.
    ///
    /// Strategy: prefetch the dependency surface OUTSIDE the tracer
    /// scope (so the load itself is not part of the observed read
    /// set). For each macro-type-dep:
    ///
    /// - Resolve `dep.import_source` via `workspace.resolve_import`
    ///   (Type-import first, ESM-import fallback) and cache the route
    ///   in `derived_raw_cache().import_routes`.
    /// - Call `ensure_indexed_ready(dep_canonical)` to publish the
    ///   dependency's `IndexedReady` into `FileArtifactStore`. Just
    ///   `ensure_loaded` is insufficient — fact lookup reads the
    ///   indexed-artifact's `facts` registry, which is only populated
    ///   by the indexed-ready materialiser.
    ///
    /// Script imports (used by the augmentation observation) reach
    /// `FileArtifactStore` via `ensure_indexed_ready` on each
    /// resolvable specifier. Unresolved specifiers (external packages
    /// without a workspace fallback) are skipped: the augmentation
    /// observation uses the index-level fingerprint snapshot rather
    /// than per-canonical artifacts and tolerates a missing canonical.
    fn prefetch_compile_tier_observation_targets(
        &self,
        owner_canonical: &str,
        script_imports: &[verter_semantic::analysis::AnalyzedImport],
        macro_type_deps: &[verter_semantic::analysis::MacroTypeDep],
        external_requests: &[ExternalSourceRequest],
    ) {
        // Test/debug-only invocation count. The cold-compute path gates
        // this prefetch to `Session`; the counter lets a routing test
        // observe that gate without a fact-rail side channel.
        #[cfg(any(test, debug_assertions))]
        COMPILE_TIER_PREFETCH_INVOCATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Owner's indexed-ready must be present so the tracer can
        // resolve owner-relative import surfaces; the owner's own
        // FileArtifactStore entry is also a producer-side dependency
        // of route observation (R26).
        let _ = self.ensure_indexed_ready(owner_canonical);

        let workspace = self.workspace();
        let mut resolved_deps = std::collections::BTreeSet::<String>::new();

        // Macro-type deps: TypeImport first, ESM fallback. Cache the
        // import-route so `resolve_import_source_to_canonical` in
        // `compile_fact_emission` finds it.
        for dep in macro_type_deps {
            let resolved = workspace
                .resolve_import(
                    owner_canonical,
                    &dep.import_source,
                    verter_workspace::ResolutionContext {
                        phase: verter_workspace::ResolvePhase::CodegenBlocker,
                        kind: verter_workspace::ResolveRequestKind::TypeImport,
                    },
                )
                .or_else(|| {
                    workspace.resolve_import(
                        owner_canonical,
                        &dep.import_source,
                        verter_workspace::ResolutionContext {
                            phase: verter_workspace::ResolvePhase::CodegenBlocker,
                            kind: verter_workspace::ResolveRequestKind::EsmImport,
                        },
                    )
                });
            if let Some(resolution) = resolved {
                self.cache_positive_import_route_result(
                    owner_canonical,
                    &dep.import_source,
                    &resolution.source_id,
                );
                if resolution.source_id != owner_canonical {
                    resolved_deps.insert(resolution.source_id);
                }
            }
        }

        // Script imports: cache the import route + indexed-ready so
        // the tracer's ImportRef + augmentation observations have
        // populated state. Type-only imports use the TypeImport
        // phase; value imports use EsmImport.
        for import in script_imports {
            let kind = if import.is_type_only {
                verter_workspace::ResolveRequestKind::TypeImport
            } else {
                verter_workspace::ResolveRequestKind::EsmImport
            };
            if let Some(resolution) = workspace.resolve_import(
                owner_canonical,
                import.source.as_str(),
                verter_workspace::ResolutionContext {
                    phase: verter_workspace::ResolvePhase::CodegenBlocker,
                    kind,
                },
            ) {
                self.cache_positive_import_route_result(
                    owner_canonical,
                    import.source.as_str(),
                    &resolution.source_id,
                );
                if resolution.source_id != owner_canonical {
                    resolved_deps.insert(resolution.source_id);
                }
            }
        }

        // External `src=` blocks. The compile-tier producer observes
        // a `FileWholeHash` of each external canonical, so each
        // external dep must reach the store before the tracer runs.
        //
        // A pre-existing import-route for the `src=` specifier is
        // AUTHORITATIVE — it was placed by `set_import_dependencies`
        // (caller-supplied exact resolution) or by an earlier
        // resolution pass. This prefetch must NOT overwrite it: an
        // aliased `src=` (`@/partials/panel.html`) does not resolve
        // through `workspace.resolve_import`, so re-resolving and
        // caching the fallback would clobber the correct caller route
        // and break external-source merging. The prefetch only
        // authors a route when none exists yet.
        for request in external_requests {
            let existing_route = self
                .derived_raw_cache()
                .get(owner_canonical)
                .and_then(|d| d.import_routes.get(&request.specifier).cloned());
            let resolved = if let Some(route) = existing_route {
                // Use the authoritative route's canonical for the
                // indexed-ready prefetch; leave the route untouched.
                route
                    .resolved_canonical_id
                    .clone()
                    .or_else(|| route.effective_target().map(str::to_string))
                    .unwrap_or_else(|| request.resolved_canonical_id.clone())
            } else {
                // No route yet — resolve through the SfcSrcAttr phase
                // and cache the result so the producer's
                // `resolve_import_source_to_canonical` finds it. Fall
                // back to the parse-time canonical when the workspace
                // cannot resolve the specifier.
                let resolved = workspace
                    .resolve_import(
                        owner_canonical,
                        &request.specifier,
                        verter_workspace::ResolutionContext {
                            phase: verter_workspace::ResolvePhase::CodegenBlocker,
                            kind: verter_workspace::ResolveRequestKind::SfcSrcAttr,
                        },
                    )
                    .map(|resolution| resolution.source_id)
                    .unwrap_or_else(|| request.resolved_canonical_id.clone());
                if !resolved.is_empty() && resolved != owner_canonical {
                    self.cache_positive_import_route_result(
                        owner_canonical,
                        &request.specifier,
                        &resolved,
                    );
                }
                resolved
            };
            if !resolved.is_empty() && resolved != owner_canonical {
                resolved_deps.insert(resolved);
            }
        }

        // Drive each resolved dep to IndexedReady so its
        // `FileArtifactStore` entry (including the `facts` registry)
        // is published before the tracer queries fact hashes. Calls
        // are idempotent / cache-hit on warm reads.
        for dep_canonical in resolved_deps {
            let _ = self.ensure_indexed_ready(&dep_canonical);
        }
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub fn ensure_compiled(
        &self,
        canonical_id: &str,
        profile: &CompileProfile,
    ) -> Result<(), HostError> {
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let profile_hash = compile_profile_hash(profile);

        // Check cache
        {
            {
                use crate::host_executor::HostSourceData;
                let snap = self.scheduler.try_get_source(&canonical).ok_or_else(|| {
                    HostError::MissingSource {
                        canonical_id: canonical.clone(),
                    }
                })?;
                let hd = snap.downcast_data::<HostSourceData>().ok_or_else(|| {
                    HostError::MissingSource {
                        canonical_id: canonical.clone(),
                    }
                })?;
                if hd.file_kind == FileKind::NonSfc {
                    return Ok(());
                }
                if let Some(cc) = self.compile_cache().get(&canonical) {
                    let soh = cc
                        .style_overrides
                        .get(&profile_hash)
                        .map(|o| o.hash)
                        .unwrap_or(0);
                    let coh = cc
                        .content_overrides
                        .get(&profile_hash)
                        .map(|o| o.layer.hash)
                        .unwrap_or(0);
                    // R3/R26/R28: the warm hit must validate the SAME
                    // predicate `get_virtual_file` / `compile_slot_is_warm`
                    // use — own-content identity (`semantic_hash`), both
                    // override hashes, AND the cross-file fact signature.
                    // `semantic_hash` only covers this canonical's own
                    // content; a cross-file dependency edit (runtime
                    // import, macro type dep, external `src=` file)
                    // surfaces solely through the fact-signature validator
                    // closure. Omitting it would serve a stale slot and
                    // return `Ok(())` without recompiling.
                    let session_node =
                        crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
                    if session_node
                        .lookup(
                            &cc,
                            profile_hash,
                            &hd.parse.semantic_hash,
                            soh,
                            coh,
                            |sig| self.compile_slot_facts_validate(sig),
                        )
                        .is_some()
                    {
                        return Ok(());
                    }
                }
            }
        }

        self.hydrate_compile_blockers(&canonical);

        // Cache miss â€” compile by requesting the Main virtual file.
        // This populates ALL cached outputs (script, template, styles, TSX, etc.)
        // for the given profile.
        let _ = self.get_virtual_file(VirtualQuery {
            raw_id: None,
            canonical_id: Some(canonical),
            node_kind: Some(VirtualNodeKind::Main),
            compile_profile: profile.clone(),
        })?;
        Ok(())
    }

    /// R3/R26/R28 warm-hit fact validator closure body.
    ///
    /// Validates every fact recorded on a non-empty
    /// [`ReadSetSignature`](crate::fact_signature_helpers::ReadSetSignature)
    /// against the host's current `HostStoreView`. A single mismatch
    /// returns `false` and the warm hit misses; the caller falls
    /// through to cold recompute.
    ///
    /// This is the validator closure passed to
    /// [`crate::cache_runtime::CompileOutputNodeFactValidatedSession::lookup`].
    /// The node owns the warm-hit gate: it refuses an overflowed
    /// carrier and short-circuits an empty fact rail (where the
    /// upstream `semantic_hash` / override-hash pre-filter is the sole
    /// gating predicate) BEFORE invoking this closure, so this method
    /// only walks a non-empty fact set.
    ///
    /// `O(signature.len())` per call. Validation reads through the
    /// `HostStoreView` snapshot captured at the start of the request;
    /// concurrent edits do NOT race against this read.
    #[inline]
    pub(crate) fn compile_slot_facts_validate(
        &self,
        signature: &crate::fact_signature_helpers::ReadSetSignature,
    ) -> bool {
        let view = self.resolver_store_view();
        use crate::resolver_core::StoreView;
        signature.facts.iter().all(|fact| view.validates(fact))
    }

    /// Read-only predicate: would `get_virtual_file(query)` for this
    /// `(canonical_id, profile)` hit the compile cache without doing any
    /// work?
    ///
    /// Mirrors the freshness predicate the writer uses inside
    /// `get_virtual_file` (`slot.semantic_hash == parse.semantic_hash
    /// && slot.style_override_hash == soh && slot.content_override_hash
    /// == coh && fact-signature validates`). The predicate stays in
    /// lockstep with the writer; if the writer's predicate ever
    /// changes, this accessor changes with it.
    ///
    /// R3 fact-validation gates the warm hit: the `slot.semantic_hash`
    /// check covers the owning canonical's own content identity, but
    /// cross-file dependency edits (e.g. `/src/types.ts` mutates while
    /// `/src/Comp.vue` is unchanged) only surface through
    /// `compile_slot_fact_signature_validates`. A consumer with a
    /// stale fact_dep_signature lookup mismatches the active view
    /// here and the predicate returns `false`, which routes the
    /// caller through cold recompute.
    pub fn compile_slot_is_warm(&self, canonical_id: &str, profile: &CompileProfile) -> bool {
        use crate::host_executor::HostSourceData;
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let profile_hash = compile_profile_hash(profile);

        let snap = match self.scheduler.try_get_source(&canonical) {
            Some(s) => s,
            None => return false,
        };
        let hd = match snap.downcast_data::<HostSourceData>() {
            Some(h) => h,
            None => return false,
        };
        let parse = &hd.parse;

        let cc = match self.compile_cache().get(&canonical) {
            Some(c) => c,
            None => return false,
        };
        let soh = cc
            .style_overrides
            .get(&profile_hash)
            .map(|o| o.hash)
            .unwrap_or(0);
        let coh = cc
            .content_overrides
            .get(&profile_hash)
            .map(|o| o.layer.hash)
            .unwrap_or(0);
        let session_node = crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
        session_node
            .lookup(&cc, profile_hash, &parse.semantic_hash, soh, coh, |sig| {
                self.compile_slot_facts_validate(sig)
            })
            .is_some()
    }

    /// Public R3/R26/R28 inspector: returns a clone of the compile
    /// slot's `fact_dep_signature` for the given `(canonical, profile)`
    /// pair, or `None` if no slot has been admitted.
    ///
    /// Used by integration tests + downstream observability to verify
    /// the producer actually recorded the cross-file fact set the
    /// consumer's read-side fact-validation oracle depends on. The
    /// returned `ReadSetSignature` exposes `.facts` (the path-precise
    /// fact rail) and `.is_overflow()` / `.is_cacheable()` directly;
    /// callers that want the raw fact slice read `.facts`.
    pub fn compile_slot_fact_dep_signature(
        &self,
        canonical_id: &str,
        profile: &CompileProfile,
    ) -> Option<crate::fact_signature_helpers::ReadSetSignature> {
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let profile_hash = compile_profile_hash(profile);
        let session_node = crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
        self.compile_cache()
            .get(&canonical)
            .and_then(|cc| session_node.peek_signature(&cc, profile_hash))
    }

    /// Build the content-addressed cache key for a
    /// [`CompileCacheMode::Content`] compile request.
    ///
    /// Every byte-determined input the compiled artifact depends on
    /// enters the key: the source canonical and its `content_hash`, the
    /// four split env-dimension hashes plus the project identity (from
    /// the per-canonical env-hash bundle), the public-API mode hash, the
    /// source-map policy hash, and the compiler / plugin version hashes.
    /// Two requests that agree on every dimension MUST produce
    /// byte-identical output, so a single content entry serves both.
    fn compile_pure_content_key(
        &self,
        canonical_id: &str,
        content_hash: Hash16,
        profile: &CompileProfile,
    ) -> crate::cache_runtime::CompileOutputPureContentKey {
        let env = self.host_view_env_hashes_for(canonical_id);
        let project_identity = self.host_view_project_identity_for(canonical_id).0;
        // Source-map emission policy projected from the profile. The
        // profile carries a single `source_map` toggle; map it onto the
        // public policy enum so two requests with different emission
        // policies never share a content entry.
        let source_map_policy = if profile.source_map {
            SourceMapPolicy::Inline
        } else {
            SourceMapPolicy::None
        };
        crate::cache_runtime::CompileOutputPureContentKey {
            canonical_id: Arc::from(canonical_id),
            content_hash,
            parse_env_hash: env.parse_env_hash,
            resolve_env_hash: env.resolve_env_hash,
            type_env_hash: env.type_env_hash,
            lib_env_hash: env.lib_env_hash,
            project_identity,
            // Compile-mode discriminator: the public cache mode PLUS the
            // full profile identity (target, ssr, force_js,
            // is_production, delimiters, …). Two Content requests for the
            // same content + env but different profiles produce different
            // output, so they must not share a content entry.
            compile_cache_mode_hash: content_mode_profile_hash(profile),
            source_map_policy_hash: source_map_policy.stable_hash(),
            compiler_version: compiler_version_hash(),
            plugin_versions: plugin_versions_hash(),
        }
    }

    /// Retrieve a compiled virtual file (script, template, style, or main bundle).
    ///
    /// On cache hit, returns immediately. On cache miss, compiles the file using
    /// `verter_compiler::compile`, caches the result, and returns the requested node.
    /// In dev mode with [`CompileErrorPolicy::DevServeLastKnownGood`], falls back
    /// to the last successful compilation when the current source has errors.
    pub fn get_virtual_file(&self, query: VirtualQuery) -> Result<VirtualFileResponse, HostError> {
        #[cfg(feature = "session_metrics")]
        self.metrics
            .virtual_loads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let (canonical_id, node_kind, raw_was_lsp) = if let Some(raw) = query.raw_id.clone() {
            let parsed = parse_raw_id(&raw).ok_or(HostError::InvalidQuery)?;
            (
                self.resolve_alias_or_canonical(&parsed.canonical_id),
                parsed.node_kind,
                parsed.was_lsp_like,
            )
        } else if let (Some(canonical), Some(node_kind)) =
            (query.canonical_id.clone(), query.node_kind.clone())
        {
            (
                self.resolve_alias_or_canonical(&canonical),
                node_kind,
                false,
            )
        } else {
            return Err(HostError::InvalidQuery);
        };

        let profile_hash = compile_profile_hash(&query.compile_profile);
        let requested_mode = query.compile_profile.requested_mode;

        // Cache hit check and compile input extraction under a single read lock.
        // This avoids cloning the full FileEntry (with all compile_slots, style_overrides, etc.)
        // on the hot path.
        struct CacheMiss {
            compile_input: CompileInput,
            fallback_last_good: Option<FxHashMap<VirtualNodeKind, CachedVirtualFile>>,
            meta: FileMeta,
            /// Captured under read lock so the compile slot is stored with the
            /// semantic_hash that was current when we decided to compile.
            semantic_hash: Hash16,
            /// Content `whole_hash` captured under the same read lock, so a
            /// `Content`-mode publish keys the content-addressed entry on the
            /// exact source version that was compiled.
            whole_hash: Hash16,
            /// The mode classification, computed once under the read lock
            /// from this request's effective eligibility surface. The
            /// classifier is the sole authority for the mode decision and
            /// gates the warm-hit consult, so it must be known BEFORE any
            /// cache read. Carried out of the block so the audit event and
            /// the compile/publish routing reuse the single classification.
            classification: crate::compile_cache_mode::CompileModeClassification,
        }

        // Capture scheduler source state at compile START for artifact commit.
        let sched_snapshot_at_start = self.scheduler.try_get_source(&canonical_id);

        let cache_miss = {
            {
                use crate::host_executor::{HostAnalysisData, HostSourceData};

                let source_snap =
                    self.scheduler
                        .try_get_source(&canonical_id)
                        .ok_or_else(|| HostError::MissingSource {
                            canonical_id: canonical_id.clone(),
                        })?;
                let hd = source_snap
                    .downcast_data::<HostSourceData>()
                    .ok_or_else(|| HostError::MissingSource {
                        canonical_id: canonical_id.clone(),
                    })?;
                let parse = &hd.parse;

                let cc_ref = self.compile_cache().get(&canonical_id);

                // Cache hit check from compile_cache
                let soh = cc_ref
                    .as_ref()
                    .and_then(|cc| cc.style_overrides.get(&profile_hash).map(|o| o.hash))
                    .unwrap_or(0);
                let coh = cc_ref
                    .as_ref()
                    .and_then(|cc| {
                        cc.content_overrides
                            .get(&profile_hash)
                            .map(|o| o.layer.hash)
                    })
                    .unwrap_or(0);

                // Build this request's effective compile input (override-
                // aware) and classify the cache mode BEFORE any warm-hit
                // consult. The classifier is the sole authority for the
                // mode decision and it gates the cache read: a request that
                // classifies to `Stateless` must not consult any host cache
                // node, and a `Content` warm hit is valid only when the
                // request actually classifies to `Content`. A request-time
                // block / style override removes the session slot but does
                // not bump `whole_hash` nor evict the content-addressed
                // entry, so consulting before classification would serve a
                // stale `Content` entry for an input the override forces to
                // downgrade. Classifying first closes that gap.
                let efs = self
                    .effective_file_state(&canonical_id, Some(profile_hash))
                    .ok_or_else(|| HostError::MissingSource {
                        canonical_id: canonical_id.clone(),
                    })?;
                let effective_meta = self
                    .effective_meta(&canonical_id, Some(profile_hash))
                    .unwrap_or_else(|| parse.meta.clone());

                let style_override_layer = cc_ref.as_ref().and_then(|cc| {
                    cc.style_overrides
                        .get(&profile_hash)
                        .map(|o| o.layer.clone())
                });
                let content_override_layer = cc_ref.as_ref().and_then(|cc| {
                    cc.content_overrides
                        .get(&profile_hash)
                        .map(|o| o.layer.clone())
                });
                let fallback_last_good = cc_ref.as_ref().and_then(|cc| {
                    let session_node =
                        crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
                    session_node.peek_last_good(cc, profile_hash)
                });

                // Style v-bind vars from raw analysis (override-independent)
                let analysis_snap = self.scheduler.try_get_analysis(&canonical_id);
                let style_analyses: Arc<Vec<verter_semantic::analysis::StyleBlockAnalysis>> =
                    analysis_snap
                        .as_ref()
                        .and_then(|a| a.downcast_data::<HostAnalysisData>())
                        .map(|ad| Arc::clone(&ad.style_analyses))
                        .unwrap_or_default();

                let compile_input = CompileInput {
                    canonical_id: canonical_id.clone(),
                    source: efs.source,
                    meta: effective_meta.clone(),
                    parse_diagnostics: parse.parse_diagnostics.clone(),
                    src_blocks: parse.src_blocks.clone(),
                    external_requests: parse.external_requests.clone(),
                    style_override_layer,
                    content_override_layer,
                    macro_type_deps: efs.script_analysis.macro_type_deps.clone(),
                    script_imports: efs.script_analysis.imports.clone(),
                    script_macros: efs.script_analysis.macros.clone(),
                    script_bindings: efs.script_analysis.bindings.clone(),
                    cached_parse: efs.cached_parse,
                    style_v_bind_vars: style_analyses
                        .iter()
                        .flat_map(|sa| {
                            sa.v_binds.iter().map(|vb| {
                                vb.expression
                                    .split('.')
                                    .next()
                                    .unwrap_or(&vb.expression)
                                    .to_string()
                            })
                        })
                        .collect(),
                };

                // Classify EXACTLY ONCE per compile, here under the read
                // lock, so `actual_mode` is known before the warm-hit
                // consult and reused by the compile / publish routing.
                // `HasModuleAugmentation` is probed ONLY when
                // `requested_mode == CompileCacheMode::Content`. The
                // closure-aware probe (`owner_has_module_augmentation_dependency`,
                // which consults the augmentation target index for every
                // module the owner can consume plus ambient / global
                // augmenters) pays a store scan, and a Session request
                // preserves Session under every reason while a Stateless
                // request is the floor that ignores all reasons (see
                // `classify_compile_mode` in `compile_cache_mode.rs`), so
                // neither consults this bit and the scan is paid only on
                // the rare explicit Content opt-in.
                let owner_has_module_augmentation = requested_mode == CompileCacheMode::Content
                    && self.owner_has_module_augmentation_dependency(&canonical_id);
                let classification = crate::compile_cache_mode::classify_compile_mode(
                    requested_mode,
                    &crate::compile_cache_mode::EligibilityInputs {
                        input: &compile_input,
                        profile: &query.compile_profile,
                        config: &self.config,
                        workspace_aliases: &self.workspace_aliases_for_canonical(&canonical_id),
                        owner_has_module_augmentation,
                    },
                );
                let actual_mode = classification.actual_mode;

                // Warm-hit consult, routed by the ACTUAL (classified) cache
                // mode. `Session` validates the fact-validated session slot;
                // `Content` peeks the pure content-addressed entry; a request
                // that classified to `Stateless` (including a downgraded
                // `Content`) bypasses both nodes. The warm hit returns the
                // node's output + diagnostics, which are identical in shape
                // across modes.
                struct WarmHit {
                    outputs: FxHashMap<VirtualNodeKind, CachedVirtualFile>,
                    diagnostics: DiagnosticsSnapshot,
                }
                let warm_hit: Option<WarmHit> = match actual_mode {
                    CompileCacheMode::Stateless => None,
                    CompileCacheMode::Session => cc_ref.as_ref().and_then(|cc| {
                        let session_node =
                            crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
                        session_node
                            .lookup(cc, profile_hash, &parse.semantic_hash, soh, coh, |sig| {
                                self.compile_slot_facts_validate(sig)
                            })
                            .map(|hit| WarmHit {
                                outputs: hit.outputs,
                                diagnostics: hit.diagnostics,
                            })
                    }),
                    CompileCacheMode::Content => {
                        let key = self.compile_pure_content_key(
                            &canonical_id,
                            parse.whole_hash,
                            &query.compile_profile,
                        );
                        self.compile_output_pure_content()
                            .peek(&key)
                            .map(|value| WarmHit {
                                outputs: value.outputs.clone(),
                                diagnostics: value.diagnostics.clone(),
                            })
                    }
                };

                if let Some(hit) = warm_hit {
                    #[cfg(feature = "session_metrics")]
                    self.metrics
                        .compile_cache_hits
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                    // Build effective meta for cache-hit render_ids.
                    let mut hit_meta = parse.meta.clone();
                    if let Some(ref cc) = cc_ref {
                        if let Some(so) = cc.style_overrides.get(&profile_hash) {
                            for (idx, lang) in so.lang_overrides.iter().enumerate() {
                                if let Some(ref l) = lang {
                                    if idx < hit_meta.style_langs.len() {
                                        hit_meta.style_langs[idx] = Some(l.clone());
                                    }
                                }
                            }
                        }
                    }

                    if let Some(found) = hit.outputs.get(&node_kind) {
                        // A warm hit is served only for the classified mode.
                        // A `Content` warm hit implies no reason fired —
                        // a reason would have downgraded the request to
                        // `Stateless`, which bypasses this consult — so a
                        // `Content` hit always carries `actual == requested`
                        // with no downgrade reason. A `Session` warm hit is
                        // served from the validated session node and may
                        // still carry `Some(reason)`: `Session` stays
                        // `Session` under every reason and retains the
                        // reasons for telemetry, so `downgrade_reason`
                        // (`first_downgrade_reason()`, below) can be
                        // `Some(reason)` while `actual == requested`.
                        return Ok(VirtualFileResponse {
                            id: render_single_id(&canonical_id, &node_kind, &hit_meta, raw_was_lsp),
                            code: found.code.clone(),
                            source_map: found.source_map.clone(),
                            lang: found.lang.clone(),
                            stale: false,
                            diagnostics: hit.diagnostics.clone(),
                            meta: found.meta.clone(),
                            cache_hit: true,
                            requested_mode,
                            actual_mode,
                            downgrade_reason: classification.first_downgrade_reason(),
                        });
                    }
                }

                drop(cc_ref);

                CacheMiss {
                    compile_input,
                    fallback_last_good,
                    meta: effective_meta,
                    semantic_hash: parse.semantic_hash,
                    whole_hash: parse.whole_hash,
                    classification,
                }
            }
        };

        let CacheMiss {
            compile_input,
            fallback_last_good,
            meta,
            semantic_hash: captured_semantic_hash,
            whole_hash: captured_whole_hash,
            classification,
        } = cache_miss;

        #[cfg(feature = "session_metrics")]
        self.metrics
            .compile_requests
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        #[cfg(feature = "session_metrics")]
        let compile_start = Instant::now();

        let style_override_hash = compile_input
            .style_override_layer
            .as_ref()
            .map(|o| o.hash)
            .unwrap_or(0);
        let content_override_hash = compile_input
            .content_override_layer
            .as_ref()
            .map(|o| o.hash)
            .unwrap_or(0);

        // The mode classification was computed once under the read lock
        // (it gated the warm-hit consult). The classifier is the sole
        // authority for the mode decision; this pipeline only consumes the
        // carried result. `Session` (the host default) stays `Session` for
        // every reason; `Content` downgrades to `Stateless` on any reason;
        // `Stateless` is the floor.
        let actual_mode = classification.actual_mode;
        let downgrade_reason = classification.first_downgrade_reason();

        // Emit the downgrade audit event at classification time, at most
        // once per compile request, only when the actual mode differs
        // from the requested mode (under the mode fold this is exactly a
        // `Content -> Stateless` downgrade). The full ordered reason set
        // is preserved on the event for telemetry even though the public
        // single-reason projection keeps only the first.
        if actual_mode != classification.requested_mode {
            crate::host_manage::push_structured_event(
                crate::component_meta_audit::StructuredAuditEvent::CompileModeDowngrade {
                    requested: classification.requested_mode.into(),
                    actual: actual_mode.into(),
                    reasons: classification
                        .downgrade_reasons
                        .iter()
                        .map(|r| (*r).into())
                        .collect(),
                },
            );
        }

        // Cold-compute prefetch: resolve + index the cross-file
        // dependency surface so the compile-tier fact tracer can observe
        // populated import-route + `IndexedReady` state. Performed
        // outside any fact-tracer scope so that load / index mutations
        // are not folded into a consumer's observed read set.
        //
        // Session-only: the tracer is installed exclusively in the
        // `Session` branch below, and a no-tracer fact call is a no-op
        // (`observe_compile_tier_dependencies` caller contract). The
        // prefetch is pure fact-observation pre-population — `Content` /
        // `Stateless` compile correctness (external `src=` resolution,
        // macro-type collection, dep sync) is produced independently by
        // `compile_entry`, so running the prefetch for those modes would
        // be load + index work nobody records.
        if actual_mode == CompileCacheMode::Session {
            self.prefetch_compile_tier_observation_targets(
                &canonical_id,
                &compile_input.script_imports,
                &compile_input.macro_type_deps,
                &compile_input.external_requests,
            );
        }

        // Compile, routed by the actual cache mode.
        //
        // `Session` installs the R3/R26/R28 fact-observation tracer: it
        // accumulates every cross-file fact (per-`Member` /
        // `MemberPresence` for macro type deps, `ImportRef` per script
        // import, `ModuleAugmentationIndexShape` per augmented specifier)
        // the compile reads, finalises a `ReadSetSignature`, and routes
        // it through `SignatureAdmission`. `Content` and `Stateless`
        // have NO fact rail, so they compile directly without the tracer
        // and never finalise a signature.
        let (compile_result, compile_admission) = if actual_mode == CompileCacheMode::Session {
            let (result, fact_read_set) = self.with_fact_tracer(|| {
                crate::compile_fact_emission::observe_compile_tier_dependencies(
                    self,
                    &canonical_id,
                    &compile_input.script_imports,
                    &compile_input.macro_type_deps,
                    &compile_input.external_requests,
                );
                // Test-only fact injection: when armed, emit `N`
                // synthetic `FileWholeHash` observations into the active
                // tracer. `N > FACT_SIGNATURE_CAP` (1024) drives the
                // tracer to `Overflow` deterministically, exercising the
                // refuse-publish-on-overflow path without a pathological
                // workspace fixture.
                let force_n = COMPILE_TEST_FORCE_OVERFLOW_OBSERVATIONS
                    .load(std::sync::atomic::Ordering::Relaxed);
                if force_n > 0 {
                    for n in 0..force_n {
                        crate::resolver_core::resolver_context::observe_fan_out(
                            crate::resolver_core::FactVersionRef::FileWholeHash {
                                canonical_id: format!("__compile_force_overflow_{n}.ts"),
                                hash: [(n & 0xff) as u8; 16],
                            },
                        );
                    }
                }
                self.compile_entry(&compile_input, &query.compile_profile)
            });
            // `Cacheable(sig)` → publish the compile-output slot through
            // the typed session node under the path-precise signature.
            // `NonCacheable` (overflow) → the session node removes any
            // prior slot and the freshly computed value is returned
            // without admitting. The caller-visible result is computed
            // independently of admission.
            let admission =
                crate::cache_runtime::SignatureAdmission::from_finalise(fact_read_set.finalise());
            (result, Some(admission))
        } else {
            // `Content` / `Stateless`: no tracer, no fact signature.
            let result = self.compile_entry(&compile_input, &query.compile_profile);
            (result, None)
        };
        let (compiled_outputs, diagnostics, stale, compiled_tsx, compiled_template_analysis) =
            match compile_result {
                Ok((outputs, diagnostics, tsx, tpl)) => (outputs, diagnostics, false, tsx, tpl),
                Err(diagnostics) => {
                    self.store_latest_diagnostics(&canonical_id, profile_hash, diagnostics.clone());
                    let policy = self.config.compile_error_policy;
                    // `fallback_last_good` is session-published output. A
                    // `Stateless` compile bypasses ALL host cache reads —
                    // including this dev-serve-last-good read-back — so it
                    // never serves the session last-good even on error.
                    // (`actual_mode == Stateless` is reached either by an
                    // explicit `Stateless` request or by a downgraded
                    // `Content` request.)
                    let serve_last_good = actual_mode != CompileCacheMode::Stateless
                        && self.config.dev_mode
                        && policy == CompileErrorPolicy::DevServeLastKnownGood;
                    if serve_last_good {
                        if let Some(last_good) = fallback_last_good.clone() {
                            (last_good, diagnostics, true, None, None)
                        } else {
                            return Err(HostError::CompileError(CompileFailure {
                                diagnostics,
                                requested_mode: classification.requested_mode,
                                actual_mode,
                                downgrade_reason,
                            }));
                        }
                    } else {
                        return Err(HostError::CompileError(CompileFailure {
                            diagnostics,
                            requested_mode: classification.requested_mode,
                            actual_mode,
                            downgrade_reason,
                        }));
                    }
                }
            };

        #[cfg(feature = "session_metrics")]
        {
            let compile_elapsed_us = compile_start.elapsed().as_micros() as u64;
            self.metrics
                .compile_time_us_total
                .fetch_add(compile_elapsed_us, std::sync::atomic::Ordering::Relaxed);
            if let Ok(mut per_profile) = self.metrics.compile_time_us_total_by_profile.lock() {
                let entry = per_profile.entry(profile_hash).or_insert(0);
                *entry = entry.saturating_add(compile_elapsed_us);
            }
            if let Ok(mut per_profile_count) = self.metrics.compile_count_by_profile.lock() {
                let entry = per_profile_count.entry(profile_hash).or_insert(0);
                *entry = entry.saturating_add(1);
            }
        }

        let last_tick = self.tick.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // The freshly compiled value, shared by the Session and Content
        // publish paths. Stateless drops it after returning the response.
        let compile_output_value = crate::cache_runtime::CompileOutputValue::from_compile_record(
            captured_semantic_hash,
            style_override_hash,
            content_override_hash,
            compiled_outputs.clone(),
            diagnostics.clone(),
            if stale {
                fallback_last_good.clone()
            } else {
                Some(compiled_outputs.clone())
            },
            compiled_tsx.clone(),
            compiled_template_analysis.clone(),
        );

        // The `latest_diagnostics` + generation bump runs for EVERY mode
        // so compile errors / warnings surface regardless of caching.
        // This is observable diagnostic state, not a compile-output
        // cache entry.
        if let Some(mut cc) = self.compile_cache().get_mut(&canonical_id) {
            cc.latest_diagnostics
                .insert(profile_hash, diagnostics.clone());
            cc.diagnostics_generation += 1;
        }

        // Publish, routed by the actual cache mode.
        match actual_mode {
            CompileCacheMode::Stateless => {
                // Bypass both typed cache nodes: publish nothing. The
                // caller still receives the freshly computed virtual
                // file below.
            }
            CompileCacheMode::Content => {
                // Publish into the content-addressed node ONLY. No fact
                // rail, no session slot, no scheduler artifact: the
                // content key's env-hash dimensions already invalidate
                // on every observable env change.
                let key = self.compile_pure_content_key(
                    &canonical_id,
                    captured_whole_hash,
                    &query.compile_profile,
                );
                let generation = self.project_type_store.current_project_generation();
                self.compile_output_pure_content().publish_content(
                    key,
                    compile_output_value,
                    generation,
                );
            }
            CompileCacheMode::Session => {
                // Route the finalised admission through the typed session
                // node. `Cacheable(sig)` publishes the slot under the
                // path-precise signature AND commits the scheduler
                // artifact snapshot — both observable warm-hit substrates
                // land together. `NonCacheable(_)` (overflow) skips both
                // and removes any prior slot so the carrier invariant
                // `present in the session slot map ⇒ admitted cache entry
                // for the current version` survives an overflowing
                // recompute after a prior successful publish.
                let admission =
                    compile_admission.expect("Session mode always finalises a SignatureAdmission");
                let is_cacheable = matches!(
                    admission,
                    crate::cache_runtime::SignatureAdmission::Cacheable(_)
                );
                if let Some(mut cc) = self.compile_cache().get_mut(&canonical_id) {
                    let session_node =
                        crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
                    session_node.publish(
                        &mut cc,
                        profile_hash,
                        admission,
                        compile_output_value,
                        last_tick,
                    );
                }

                if is_cacheable {
                    // Persist raw template analysis on DerivedRawState
                    // (the profileless source-derived cache). Only for
                    // non-override compiles.
                    if compiled_template_analysis.is_some()
                        && compile_input.content_override_layer.is_none()
                    {
                        let mut derived_ref = self
                            .derived_raw_cache()
                            .entry(canonical_id.clone())
                            .or_default();
                        derived_ref.value_mut().raw_template_analysis =
                            compiled_template_analysis.clone().map(Arc::new);
                    }

                    // Commit to scheduler artifact snapshot (scheduler
                    // path only). Gated on `Cacheable` admission so the
                    // carrier invariant holds at the artifact substrate
                    // layer too — a refused compile must not be observable
                    // via `try_get_artifact` or pending Artifact requests.
                    if let Some(ref snap) = sched_snapshot_at_start {
                        self.scheduler.commit_artifact(
                            &canonical_id,
                            profile_hash,
                            verter_scheduler::node::ArtifactSnapshot {
                                generation: snap.generation,
                                profile_hash,
                                data: Arc::new(crate::host_executor::HostArtifactData {
                                    outputs: compiled_outputs.clone(),
                                    diagnostics: diagnostics.clone(),
                                }),
                            },
                        );
                    }
                } else {
                    // Refused admission. Symmetrically evict any prior
                    // scheduler artifact snapshot so `try_get_artifact`
                    // and pending Artifact requests cannot return a stale
                    // result on the companion warm-hit substrate; no fresh
                    // artifact is committed.
                    //
                    // The eviction is gated on the compile's
                    // start-of-compile generation captured in
                    // `sched_snapshot_at_start`: a slow refused compile
                    // that started at generation N can race with a fast
                    // successful compile at N+k that already committed a
                    // newer artifact, and an unconditional evict would
                    // clobber it. Passing the captured start generation as
                    // `max_generation` makes the eviction symmetric with
                    // `commit_artifact`'s own node-generation rejection.
                    if let Some(ref snap) = sched_snapshot_at_start {
                        self.scheduler.remove_artifact_if_not_newer_than(
                            &canonical_id,
                            profile_hash,
                            snap.generation,
                        );
                    }
                }
            }
        }

        // Write per-profile state to files (WASM path only).

        let found =
            compiled_outputs
                .get(&node_kind)
                .ok_or_else(|| HostError::MissingVirtualNode {
                    canonical_id: canonical_id.clone(),
                })?;

        Ok(VirtualFileResponse {
            id: render_single_id(&canonical_id, &node_kind, &meta, raw_was_lsp),
            code: found.code.clone(),
            source_map: found.source_map.clone(),
            lang: found.lang.clone(),
            stale,
            diagnostics,
            meta: found.meta.clone(),
            cache_hit: false,
            requested_mode: classification.requested_mode,
            actual_mode,
            downgrade_reason,
        })
    }

    /// List all virtual node kinds for a file (Main, Script, Template, Style, Custom).
    pub fn list_virtual_files(&self, canonical_id: &str) -> Vec<VirtualNodeKind> {
        self.list_virtual_nodes(canonical_id)
    }

    /// Retrieve the combined TSX output for LSP type checking.
    ///
    /// Returns the IDE code (TSX or JSX) and optional source map for the given file and profile.
    /// This is a dedicated API separate from the virtual file system, since IDE
    /// output is only consumed by the LSP and playground, never by bundlers.
    pub fn get_ide(&self, canonical_id: &str, profile: &CompileProfile) -> Option<IdeResponse> {
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let profile_hash = compile_profile_hash(profile);

        {
            if self.is_canonical_evicted(&canonical) {
                return None;
            }
            let cc = self.compile_cache().get(&canonical)?;
            let session_node = crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
            let tsx = session_node.peek_tsx(&cc, profile_hash)?;
            Some(IdeResponse {
                code: tsx.code.clone(),
                source_map: tsx.source_map.clone(),
                is_jsx: tsx.is_jsx,
                destructured_block: tsx.destructured_block.clone(),
            })
        }
    }

    /// Generate public API output for a Vue SFC â€” minimal TypeScript declarations.
    ///
    /// Unlike [`get_ide`](Self::get_ide), this does NOT require a prior
    /// [`get_virtual_file`](Self::get_virtual_file) call. It performs
    /// macro-only extraction (OXC parse â†’ defineProps/Emits/Model/Options)
    /// and generates a `ComponentPublicInstance`-based declaration.
    ///
    /// Returns `None` if the file is not in the host or not a Vue SFC.
    pub fn get_public_api(&self, canonical_id: &str) -> Option<TscResponse> {
        self.get_public_api_with_mode(canonical_id, PublicApiMode::Public, None)
    }

    /// Generate public API output for a Vue SFC using the requested surface mode.
    ///
    /// `PublicApiMode::Public` matches the default application-facing instance shape.
    /// `PublicApiMode::Testing` exposes internal `<script setup>` bindings in a
    /// Vue Test Utils-like debug surface.
    ///
    /// When `profile` is provided, script/content overrides for that compile
    /// profile are reflected in the generated API surface.
    pub fn get_public_api_with_mode(
        &self,
        canonical_id: &str,
        mode: PublicApiMode,
        profile: Option<&CompileProfile>,
    ) -> Option<TscResponse> {
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let profile_hash = profile.map(compile_profile_hash);

        if self.is_canonical_evicted(&canonical) {
            return None;
        }

        let (source, file_kind, macro_type_deps, script_imports, cached_extract, whole_hash) = {
            let efs = self.effective_file_state(&canonical, profile_hash)?;
            let file_kind = self.scheduler.try_get_source(&canonical).and_then(|snap| {
                snap.downcast_data::<crate::host_executor::HostSourceData>()
                    .map(|hd| hd.file_kind)
            })?;
            if file_kind != FileKind::VueSfc {
                return None;
            }
            // cached_tsc_extract lives on DerivedRawState (D48 split).
            let cached = self.derived_raw_cache().get(&canonical).and_then(|cc| {
                cc.cached_tsc_extract.as_ref().and_then(|(hash, extract)| {
                    if *hash == efs.whole_hash {
                        Some(Arc::clone(extract))
                    } else {
                        None
                    }
                })
            });
            (
                efs.source,
                file_kind,
                efs.script_analysis.macro_type_deps.clone(),
                efs.script_analysis.imports.clone(),
                cached,
                efs.whole_hash,
            )
        };

        if file_kind != FileKind::VueSfc {
            return None;
        }
        // Derive component name from canonical_id: last path segment, strip .vue extension.
        let component_name = canonical
            .rsplit('/')
            .next()
            .unwrap_or(&canonical)
            .trim_end_matches(".vue")
            .to_string();
        // External-macro-type collection. The collector iterates only
        // `macro_type_deps`; with none, it returns `(None, vec![],
        // empty_set)`, so skip building the resolver context + collector
        // entirely and substitute the empty result. The transitive set
        // is then EMPTY, which the sync below clears unconditionally.
        let (external_types, transitive_macro_type_deps) = if macro_type_deps.is_empty() {
            (None, std::collections::BTreeSet::<String>::new())
        } else {
            let store_view = self.resolver_store_view();
            let overlay =
                std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
            let host_ctx =
                crate::resolver_core::HostResolverContext::new(self, &store_view, overlay);
            let ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
            let (external_types, _, transitive) = self.collect_external_types_from_loaded_files(
                ctx,
                &canonical,
                &macro_type_deps,
                &script_imports,
                profile_hash,
            );
            (external_types, transitive)
        };
        // Unconditional: `replace_semantic_transitive(canonical, {})`
        // CLEARS the semantic axis when the set is empty (closes F15).
        self.sync_transitive_macro_type_dependencies(&canonical, &transitive_macro_type_deps);
        let tsc_mode = match mode {
            PublicApiMode::Public => verter_compiler::tsc::TscMode::Public,
            PublicApiMode::Testing => verter_compiler::tsc::TscMode::Testing,
        };

        // Try cached extract path: avoids re-parsing SFC + OXC on cache hit.
        let extract = if let Some(cached) = cached_extract {
            cached
        } else if let Some(fresh) = verter_compiler::tsc::extract_tsc_state(
            &source,
            &component_name,
            &verter_compiler::tsc::TscExtractOptions {
                filename: Some(canonical.clone()),
            },
        ) {
            let arc = Arc::new(fresh);
            {
                // cached_tsc_extract lives on DerivedRawState (D48 split).
                let mut derived_ref = self
                    .derived_raw_cache()
                    .entry(canonical.clone())
                    .or_default();
                derived_ref.value_mut().cached_tsc_extract = Some((whole_hash, Arc::clone(&arc)));
            }

            arc
        } else {
            // No <script setup> â€” fall through to direct path for empty stub
            let tsc_out = verter_compiler::tsc::generate_tsc_output_with_options(
                &source,
                &component_name,
                &verter_compiler::tsc::TscGenOptions {
                    conditional_root_narrowing: false,
                    filename: Some(canonical.clone()),
                    external_types,
                    mode: tsc_mode,
                },
            );
            return Some(TscResponse {
                code: Arc::from(tsc_out.code),
                source_map: if tsc_out.source_map.is_empty() {
                    None
                } else {
                    Some(Arc::from(tsc_out.source_map))
                },
            });
        };

        let tsc_out = verter_compiler::tsc::generate_tsc_from_state(
            &extract,
            &source,
            &component_name,
            tsc_mode,
            external_types.as_ref(),
        );
        Some(TscResponse {
            code: Arc::from(tsc_out.code),
            source_map: if tsc_out.source_map.is_empty() {
                None
            } else {
                Some(Arc::from(tsc_out.source_map))
            },
        })
    }

    /// Store diagnostics from a failed compile without triggering recompilation.
    pub(crate) fn store_latest_diagnostics(
        &self,
        canonical_id: &str,
        profile_hash: u64,
        diagnostics: DiagnosticsSnapshot,
    ) {
        if let Some(mut cc) = self.compile_cache().get_mut(canonical_id) {
            cc.latest_diagnostics.insert(profile_hash, diagnostics);
            cc.diagnostics_generation += 1;
        }
    }

    #[allow(clippy::type_complexity)]
    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn compile_entry(
        &self,
        snapshot: &CompileInput,
        profile: &CompileProfile,
    ) -> Result<
        (
            FxHashMap<VirtualNodeKind, CachedVirtualFile>,
            DiagnosticsSnapshot,
            Option<CachedTsx>,
            Option<verter_semantic::analysis::template::TemplateAnalysisSnapshot>,
        ),
        DiagnosticsSnapshot,
    > {
        let mut diagnostics = snapshot.parse_diagnostics.clone();

        let mut merged_source = snapshot.source.to_string();
        if !snapshot.src_blocks.is_empty() {
            let ext_sources = {
                let mut map = FxHashMap::default();
                for req in &snapshot.external_requests {
                    if let Some(dep_source) = self.resolve_dep_source(
                        &snapshot.canonical_id,
                        &req.resolved_canonical_id,
                        &req.specifier,
                    ) {
                        map.insert(req.resolved_canonical_id.clone(), dep_source);
                    }
                }
                map
            };

            for (idx, req) in snapshot.external_requests.iter().enumerate() {
                if !ext_sources.contains_key(&req.resolved_canonical_id) {
                    let span = snapshot.src_blocks.get(idx).map(|block| {
                        verter_span::Span::new(block.tag_open_start, block.tag_open_end)
                    });
                    diagnostics =
                        diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
                            severity: HostSeverity::Error,
                            code: "HOST_MISSING_EXTERNAL_SOURCE".to_string(),
                            message: format!(
                                "missing external source '{}' for '{}'",
                                req.specifier, snapshot.canonical_id
                            ),
                            span,
                        }]));
                }
            }

            if diagnostics.has_errors {
                return Err(diagnostics);
            }

            merged_source =
                merge_external_sources(&merged_source, &snapshot.src_blocks, &ext_sources);
        }

        let alloc = Allocator::new();
        let core_opts = CodegenOptions {
            filename: profile
                .filename
                .clone()
                .or_else(|| Some(snapshot.canonical_id.clone())),
            is_production: profile.is_production,
            // Host always assembles a standalone `function render()` via
            // assemble_main_module, so inline mode must be off â€” otherwise the
            // template emits bare identifiers (missing `$setup.` prefix).
            inline: Some(false),
            component_id: profile.component_id.clone(),
            delimiters: profile.delimiters.clone(),
            custom_elements: profile.custom_elements.clone(),
            comments: profile.comments,
            runtime_module_name: profile.runtime_module_name.clone(),
            types_module_name: profile.types_module_name.clone(),
            target: profile.target,
            embed_ambient_types: profile.embed_ambient_types,
            conditional_root_narrowing: profile.conditional_root_narrowing,
            strict_slots: profile.strict_slots,
            ..CodegenOptions::default()
        };

        let mut unresolved_macro_type_diags = Vec::new();
        let profile_hash = compile_profile_hash(profile);

        // External-macro-type collection. The collector iterates only
        // `macro_type_deps`; with none, it returns `(None, vec![],
        // empty_set)`, so skip building the resolver context + collector
        // entirely and substitute the empty result. The transitive set
        // is then EMPTY, which the sync below clears unconditionally.
        let (external_types, missing_macro_type_diags, transitive_macro_type_deps) =
            if snapshot.macro_type_deps.is_empty() {
                (
                    None,
                    Vec::new(),
                    std::collections::BTreeSet::<String>::new(),
                )
            } else {
                let store_view = self.resolver_store_view();
                let overlay =
                    std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
                let host_ctx =
                    crate::resolver_core::HostResolverContext::new(self, &store_view, overlay);
                let ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
                self.collect_external_types_from_loaded_files(
                    ctx,
                    &snapshot.canonical_id,
                    &snapshot.macro_type_deps,
                    &snapshot.script_imports,
                    Some(profile_hash),
                )
            };
        // Unconditional: `replace_semantic_transitive(canonical, {})`
        // CLEARS the semantic axis when the set is empty (closes F15).
        self.sync_transitive_macro_type_dependencies(
            &snapshot.canonical_id,
            &transitive_macro_type_deps,
        );
        unresolved_macro_type_diags.extend(missing_macro_type_diags);

        if !unresolved_macro_type_diags.is_empty() {
            diagnostics =
                diagnostics.merge(DiagnosticsSnapshot::from_vec(unresolved_macro_type_diags));
            return Err(diagnostics);
        }

        let scope = self.config.effective_scope();
        let verter_opts = VerterCompileOptions {
            force_vapor: profile.force_vapor,
            force_js: profile.force_js,
            source_map: profile.source_map,
            ssr: profile.ssr,
            external_types,
            extract_template_data: scope.needs_template_analysis(),
            prop_constness_overrides: None, // TODO: populated by cross-file optimizer,
            style_v_bind_vars: snapshot.style_v_bind_vars.clone(),
        };

        // Reuse cached parse when source wasn't modified by external src= merging
        // and no custom delimiters/elements that would change parse behavior.
        let can_use_cache = snapshot.src_blocks.is_empty()
            && profile.delimiters.is_none()
            && profile.custom_elements.is_none();

        let compiled = if can_use_cache {
            if let Some(ref cached) = snapshot.cached_parse {
                compile_from_parsed(&merged_source, cached, &core_opts, &verter_opts, &alloc)
            } else {
                compile_sfc(&merged_source, &core_opts, &verter_opts, &alloc)
            }
        } else {
            compile_sfc(&merged_source, &core_opts, &verter_opts, &alloc)
        };

        let mut compile_diags = diagnostics.clone();
        if !compiled.errors.is_empty() {
            compile_diags = compile_diags.merge(DiagnosticsSnapshot::from_vec(
                compiled
                    .errors
                    .iter()
                    .map(|d| HostDiagnostic {
                        severity: match d.severity {
                            verter_compiler::compile::CompileDiagnosticSeverity::Error => {
                                HostSeverity::Error
                            }
                            verter_compiler::compile::CompileDiagnosticSeverity::Warning => {
                                HostSeverity::Warning
                            }
                            verter_compiler::compile::CompileDiagnosticSeverity::Info => {
                                HostSeverity::Info
                            }
                        },
                        code: d.code.clone(),
                        message: d.message.clone(),
                        span: d.span,
                    })
                    .collect(),
            ));
        }

        if compile_diags.has_errors {
            return Err(compile_diags);
        }

        let mut outputs = FxHashMap::default();

        let main_code =
            assemble_main_module(&snapshot.canonical_id, &compiled, &snapshot.meta, profile);
        outputs.insert(
            VirtualNodeKind::Main,
            CachedVirtualFile {
                code: Arc::from(main_code),
                source_map: None,
                lang: Some(if profile.force_js {
                    "js".to_string()
                } else {
                    snapshot
                        .meta
                        .script_lang
                        .as_deref()
                        .unwrap_or("js")
                        .to_string()
                }),
                meta: VirtualMeta {
                    scope_id: if compiled.scope_id.is_empty() {
                        None
                    } else {
                        Some(compiled.scope_id.clone())
                    },
                    ..VirtualMeta::default()
                },
            },
        );

        if let Some(script) = compiled.script {
            outputs.insert(
                VirtualNodeKind::Script,
                CachedVirtualFile {
                    code: Arc::from(script.code),
                    source_map: if script.source_map.is_empty() {
                        None
                    } else {
                        Some(Arc::from(script.source_map))
                    },
                    lang: Some("ts".to_string()),
                    meta: VirtualMeta::default(),
                },
            );
        }

        if let Some(template) = compiled.template {
            let code = if template.imports.is_empty() {
                template.code
            } else {
                let runtime = profile.runtime_module_name.as_deref().unwrap_or("vue");
                let specifiers: Vec<String> = template
                    .imports
                    .iter()
                    .map(|name| format_import_specifier(name))
                    .collect();
                format!(
                    "import {{ {} }} from \"{}\"\n{}",
                    specifiers.join(", "),
                    runtime,
                    template.code,
                )
            };
            outputs.insert(
                VirtualNodeKind::Template,
                CachedVirtualFile {
                    code: Arc::from(code),
                    source_map: if template.source_map.is_empty() {
                        None
                    } else {
                        Some(Arc::from(template.source_map))
                    },
                    lang: Some("tsx".to_string()),
                    meta: VirtualMeta::default(),
                },
            );
        }

        let style_layer = snapshot.style_override_layer.as_ref();

        for (i, style) in compiled.styles.into_iter().enumerate() {
            let override_entry = style_layer.and_then(|layer| layer.by_index.get(&i));
            outputs.insert(
                VirtualNodeKind::Style { index: i },
                CachedVirtualFile {
                    code: override_entry
                        .map(|e| e.code.clone())
                        .unwrap_or_else(|| Arc::from(style.code)),
                    source_map: override_entry.and_then(|e| e.source_map.clone()),
                    lang: Some(style.lang.unwrap_or_else(|| "css".to_string())),
                    meta: VirtualMeta {
                        style_index: Some(i),
                        ..VirtualMeta::default()
                    },
                },
            );
        }

        for (i, block) in compiled.custom_blocks.into_iter().enumerate() {
            outputs.insert(
                VirtualNodeKind::Custom { index: i },
                CachedVirtualFile {
                    code: Arc::from(block.content),
                    source_map: None,
                    lang: snapshot.meta.custom_langs.get(i).cloned().flatten(),
                    meta: VirtualMeta {
                        custom_index: Some(i),
                        block_type: Some(block.block_type),
                        ..VirtualMeta::default()
                    },
                },
            );
        }

        // Combined IDE output (TSX/JSX) for LSP type checking â€” stored separately, not as virtual file
        let cached_tsx = compiled.tsx.map(|tsx| CachedTsx {
            code: Arc::from(tsx.code),
            source_map: if tsx.source_map.is_empty() {
                None
            } else {
                Some(Arc::from(tsx.source_map))
            },
            is_jsx: tsx.is_jsx,
            destructured_block: tsx.destructured_block,
        });

        // Convert raw template data into analysis types when available
        let template_analysis = compiled.template_data.as_ref().map(|raw| {
            // Build script import pairs for component â†’ source resolution
            let (all_imports, binding_class_unions, props_binding_name) = template_converter_inputs(
                &snapshot.script_imports,
                &snapshot.script_macros,
                &snapshot.script_bindings,
            );
            crate::template_convert::convert_raw_to_analysis(
                raw,
                &all_imports,
                &binding_class_unions,
                props_binding_name.as_deref(),
            )
        });

        Ok((outputs, compile_diags, cached_tsx, template_analysis))
    }
}
