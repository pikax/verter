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
use crate::compile::{assemble_vue_main_module, merge_external_sources};
use crate::hash::compile_profile_hash;
use crate::id::{parse_raw_id, render_ids, render_single_id};
use crate::types::*;
use crate::VerterHost;
use oxc_allocator::Allocator;
use verter_compiler::compile::format_import_specifier;
use verter_compiler::framework_common::{
    CompileUnsupported, RuntimeCompileOptions, RuntimeDiagnosticSeverity,
};

/// The render-only `Main` output of the
/// [`crate::host_compile::CompileManyTarget::RuntimeRender`] lane: the
/// assembled `_sfc_main` module bytes, its optional source map, and the
/// soft (warning-severity) diagnostics of a SUCCESSFUL render.
pub(crate) struct RenderOnlyMain {
    pub(crate) code: Arc<str>,
    pub(crate) source_map: Option<Arc<str>>,
    /// The `Main` module language (`"ts"` / `"js"` / `"jsx"`), derived
    /// identically to the HostBacked `get_virtual_file` `Main` node so the
    /// bundler consumer (vite sub-request routing) sees the same value.
    pub(crate) lang: Option<String>,
    pub(crate) diagnostics: Vec<HostDiagnostic>,
}

/// Host-scoped RAII guard that arms and clears the per-host compile-tier
/// fact-injection knob [`VerterHost::compile_force_overflow_observations`].
///
/// Test setup calls [`CompileForceOverflowGuard::arm`] with the desired
/// observation count; the guard borrows the host and clears the host's
/// field on drop so a panic / early return does not leak the forced
/// state. The knob is per-host, so arming it on one host never poisons a
/// concurrent compile on a different host running on another test thread.
#[doc(hidden)]
#[cfg(any(test, debug_assertions))]
pub struct CompileForceOverflowGuard<'h> {
    host: &'h VerterHost,
}

#[cfg(any(test, debug_assertions))]
impl<'h> CompileForceOverflowGuard<'h> {
    /// Set `host`'s forced observation count to `n` and return the guard.
    pub(crate) fn arm(host: &'h VerterHost, n: usize) -> Self {
        host.compile_force_overflow_observations
            .store(n, std::sync::atomic::Ordering::Relaxed);
        Self { host }
    }
}

#[cfg(any(test, debug_assertions))]
impl Drop for CompileForceOverflowGuard<'_> {
    fn drop(&mut self) {
        self.host
            .compile_force_overflow_observations
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Reset `host`'s compile-tier prefetch invocation counter
/// [`VerterHost::compile_tier_prefetch_invocations`] to zero. Test setup
/// calls this immediately before a cold compute so the post-compute read
/// reflects only that compute's prefetch invocations, independent of any
/// earlier compile on the same host.
#[doc(hidden)]
#[cfg(any(test, debug_assertions))]
pub fn reset_compile_tier_prefetch_invocations(host: &VerterHost) {
    host.compile_tier_prefetch_invocations
        .store(0, std::sync::atomic::Ordering::Relaxed);
}

/// Current value of `host`'s
/// [`VerterHost::compile_tier_prefetch_invocations`]. Reads the relaxed
/// atomic; pair with [`reset_compile_tier_prefetch_invocations`] around a
/// single cold compute for a deterministic observation.
#[doc(hidden)]
#[cfg(any(test, debug_assertions))]
pub fn compile_tier_prefetch_invocations(host: &VerterHost) -> usize {
    host.compile_tier_prefetch_invocations
        .load(std::sync::atomic::Ordering::Relaxed)
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

/// What a compile request demands of the shared compile result.
///
/// The shared compile (`ensure_compile_artifacts`) produces the WHOLE
/// artifact set — every virtual node PLUS the IDE `CachedTsx` — in one pass;
/// the demand is checked AFTER that shared result. `get_virtual_file` demands
/// a specific virtual node; the IDE-ensure path demands the IDE projection
/// WITHOUT requesting any virtual node (notably NOT `Main`), so a carrier that
/// projects only an IDE surface (Svelte) satisfies it without a runtime
/// `Main`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompileDemand {
    /// A specific virtual node (the `get_virtual_file` projection target).
    VirtualNode(VirtualNodeKind),
    /// The IDE (`CachedTsx`) projection — satisfied iff the served result
    /// carries a `tsx`. NEVER routed through `VirtualNode(Main)`.
    Ide,
}

/// The shared compile result `ensure_compile_artifacts` returns: the full
/// served virtual-node map, the IDE `CachedTsx` (populated even when no
/// `Main` node exists), and the request metadata callers project from.
pub(crate) struct CompileServe {
    /// Per-virtual-node-kind outputs for this `(canonical, profile)`.
    pub(crate) outputs: FxHashMap<VirtualNodeKind, CachedVirtualFile>,
    /// The combined IDE output, present when the compile produced one.
    pub(crate) tsx: Option<CachedTsx>,
    /// The effective file meta for `render_single_id`.
    pub(crate) meta: FileMeta,
    /// The diagnostics snapshot from this serve.
    pub(crate) diagnostics: DiagnosticsSnapshot,
    /// Whether this serve fell back to a stale last-known-good output.
    pub(crate) stale: bool,
    /// Whether the serve was a cache hit (no fresh compile).
    pub(crate) cache_hit: bool,
    /// The caller-requested cache mode.
    pub(crate) requested_mode: CompileCacheMode,
    /// The cache mode the runtime actually ran under.
    pub(crate) actual_mode: CompileCacheMode,
    /// The first downgrade reason, when one fired.
    pub(crate) downgrade_reason: Option<DowngradeReason>,
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
        // Capture-before-resolve: the positive-route stamps below reflect
        // the file set the resolutions ran under, never a later one (a
        // mutation racing this hydration leaves the stamps conservatively
        // stale — a harmless re-resolve, never forged currency).
        let resolved_at_generation = self.ws().content_generation();

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
                        resolved_at_generation,
                        verter_workspace::ResolveRequestKind::SfcSrcAttr,
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
                        resolved_at_generation,
                        verter_workspace::ResolveRequestKind::TypeImport,
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
                                resolved_at_generation,
                                verter_workspace::ResolveRequestKind::EsmImport,
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
    /// - Call `ensure_indexed_ready_serve(dep_canonical)` to publish the
    ///   dependency's `IndexedReady` into `FileArtifactStore`. Just
    ///   `ensure_loaded` is insufficient — fact lookup reads the
    ///   indexed-artifact's `facts` registry, which is only populated
    ///   by the indexed-ready materialiser.
    ///
    /// Script imports (used by the augmentation observation) reach
    /// `FileArtifactStore` via `ensure_indexed_ready_serve` on each
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
        // this prefetch to `Session`; the per-host counter lets a routing
        // test observe that gate without a fact-rail side channel.
        #[cfg(any(test, debug_assertions))]
        self.compile_tier_prefetch_invocations
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Owner's indexed-ready must be present so the tracer can
        // resolve owner-relative import surfaces; the owner's own
        // FileArtifactStore entry is also a producer-side dependency
        // of route observation (R26).
        let _ = self.ensure_indexed_ready_serve(owner_canonical);

        let workspace = self.workspace();
        let mut resolved_deps = std::collections::BTreeSet::<String>::new();
        // Capture-before-resolve: the positive-route stamps below reflect
        // the file set the resolutions ran under, never a later one.
        let resolved_at_generation = self.ws().content_generation();

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
                .map(|resolution| (resolution, verter_workspace::ResolveRequestKind::TypeImport))
                .or_else(|| {
                    workspace
                        .resolve_import(
                            owner_canonical,
                            &dep.import_source,
                            verter_workspace::ResolutionContext {
                                phase: verter_workspace::ResolvePhase::CodegenBlocker,
                                kind: verter_workspace::ResolveRequestKind::EsmImport,
                            },
                        )
                        .map(|resolution| {
                            (resolution, verter_workspace::ResolveRequestKind::EsmImport)
                        })
                });
            if let Some((resolution, resolved_kind)) = resolved {
                self.cache_positive_import_route_result(
                    owner_canonical,
                    &dep.import_source,
                    &resolution.source_id,
                    resolved_at_generation,
                    resolved_kind,
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
                    resolved_at_generation,
                    kind,
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
        // Route-source discipline — the per-entry freshness oracle
        // (`import_route_entry_is_generation_current`) decides whether a
        // pre-existing route may answer:
        //
        // * An UNSTAMPED route is caller-authoritative
        //   (`set_import_dependencies` — e.g. an aliased `src=`
        //   (`@/partials/panel.html`) only the embedder's resolver can
        //   map): served until replaced, never overwritten here.
        // * A STAMPED route is a host memo this prefetch (or the
        //   blocker hydration) wrote on an earlier compile: served only
        //   while its capture-before-resolve stamp matches the live
        //   `content_generation`. A stale memo means the dependency
        //   file set moved since the memo resolved — the `SfcSrcAttr`
        //   resolution may have retargeted — so it is treated as ABSENT
        //   and re-resolved + re-stamped. Serving it would suppress the
        //   retarget AND misattribute the compile-tier whole-hash
        //   observation to the retargeted-away canonical (the merge
        //   resolves live, the observation resolves through this memo).
        // * A generation-current known-miss (caller-pushed) keeps the
        //   parse-time-canonical fallback below; a stale one re-resolves.
        let live_generation = self.ws().content_generation();
        for request in external_requests {
            let existing_route = self.derived_raw_cache().get(owner_canonical).and_then(|d| {
                let route = d.import_routes.get(&request.specifier)?;
                d.import_route_entry_is_generation_current(
                    &request.specifier,
                    route,
                    live_generation,
                )
                .then(|| route.clone())
            });
            let resolved = if let Some(route) = existing_route {
                // Generation-current (or caller-authoritative) route: use
                // its canonical for the indexed-ready prefetch and leave
                // the entry untouched.
                route
                    .resolved_canonical_id
                    .clone()
                    .or_else(|| route.effective_target().map(str::to_string))
                    .unwrap_or_else(|| request.resolved_canonical_id.clone())
            } else {
                // No current route — resolve through the SfcSrcAttr lane
                // and cache the result (stamped with the pre-resolve
                // generation capture) so the producer's
                // `resolve_import_source_to_canonical` finds it. When the
                // workspace cannot resolve the specifier, the parse-time
                // canonical answers and is memoized under the same stamp:
                // the resolution attempt DID run against this file set and
                // the memo records its fallback decision; any later
                // file-set move stales the stamp and re-runs the attempt,
                // so a specifier that becomes resolvable repairs itself.
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
                        resolved_at_generation,
                        verter_workspace::ResolveRequestKind::SfcSrcAttr,
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
            let _ = self.ensure_indexed_ready_serve(&dep_canonical);
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
                // Framework-CARRIER gate: the compile path behind this
                // validator is the carrier's IDE projection (Vue OR Svelte —
                // every carrier with a registered compiler). A non-carrier
                // (plain script) never reaches it — its source execution
                // rejects with the typed unsupported-language error first.
                if !hd.file_language.is_framework_carrier() {
                    return Ok(());
                }
                // TOP-LEVEL warm validator: a compile warm hit returns
                // `Ok(())` (skip recompile) with NO outer publish /
                // is_stable fence. Validate ONLY against a proven-`Current`
                // view; a known-stale `StoreViewRead::ReturnOnly` snapshot
                // (the manager could not prove the view current under
                // churn) misses to cold — fall through to the recompile
                // below, whose own request driver re-fences promotion.
                //
                // The expensive store-view read is threaded through the
                // `acquire_view` callback `lookup` invokes ONLY after its
                // cheap predicates (slot present for this profile, carrier
                // cacheable, semantic/override hashes match) confirm there
                // is a candidate slot worth validating. A cold miss (no
                // `ProfileState`, or a present `ProfileState` with no slot
                // for this profile_hash) and a hash mismatch both fall
                // through to recompile WITHOUT building a full-workspace
                // store-view snapshot.
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
                            || {
                                // Test-only: count store-view reads that
                                // actually happened — i.e. AFTER the cheap
                                // predicates passed. A cold/profile/hash miss
                                // never reaches this callback, so the counter
                                // stays flat on those paths.
                                #[cfg(test)]
                                crate::resolver_store::record_compile_warm_validation_view_read();
                                self.resolver_store_view_read().current()
                            },
                            |current_view, sig| self.compile_slot_facts_validate(current_view, sig),
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
    /// `O(signature.len())` per call.
    ///
    /// Accepts ONLY a proven-`Current` view
    /// ([`crate::resolver_store::CurrentHostStoreView`]): the compile warm
    /// hit returns the cached compile output to the caller with NO outer
    /// publish / is_stable fence, so a known-stale
    /// `StoreViewRead::ReturnOnly` snapshot must NEVER reach this validator
    /// — it would validate a cached slot's cross-file `fact_versions`
    /// against already-mutated dependency state (`old == old`) and serve a
    /// stale compile output under churn. The `Current` proof is obtained at
    /// the warm-hit call sites, which miss to cold on a non-current read.
    #[inline]
    pub(crate) fn compile_slot_facts_validate(
        &self,
        current_view: &crate::resolver_store::CurrentHostStoreView,
        signature: &crate::fact_signature_helpers::ReadSetSignature,
    ) -> bool {
        let view = current_view.view();
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
        // Mirror the writer's warm-hit gate exactly. The store-view read
        // is threaded through `acquire_view`, which `lookup` invokes ONLY
        // after the cheap slot-present + carrier + hash predicates pass —
        // a profile-slot miss or hash mismatch reports "not warm" without
        // building a workspace snapshot. A non-current read
        // (`StoreViewRead::ReturnOnly`) can never serve a sound warm hit,
        // so `acquire_view` yields `None` there and the predicate reports
        // "not warm" — the consumer would route through cold recompute.
        let session_node = crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
        session_node
            .lookup(
                &cc,
                profile_hash,
                &parse.semantic_hash,
                soh,
                coh,
                || {
                    #[cfg(test)]
                    crate::resolver_store::record_compile_warm_validation_view_read();
                    self.resolver_store_view_read().current()
                },
                |current_view, sig| self.compile_slot_facts_validate(current_view, sig),
            )
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

    /// Render-only `Main` output for the
    /// [`crate::host_compile::CompileManyTarget::RuntimeRender`] lane:
    /// byte-identical `Main` bytes to the `HostBacked` wrapper, produced
    /// through the SAME shared substrate and host-side `Main` assembly,
    /// without the per-file session-wrapper overhead. `diagnostics` carries
    /// only the soft (warning-severity) diagnostics of a SUCCESSFUL render.
    pub(crate) fn render_only_main(
        &self,
        canonical_id: &str,
        profile: &CompileProfile,
    ) -> Result<RenderOnlyMain, HostError> {
        let canonical = self.resolve_alias_or_canonical(canonical_id);

        // The SAME profile hash `apply_block_overrides` / `get_virtual_file`
        // key request-time block / style overrides under. The bundler's
        // preprocessor path (Pug / CoffeeScript templates+scripts, custom
        // blocks, non-Vite styles) stores overrides for this profile
        // immediately before rendering, so the render lane must read the
        // override-aware effective state — otherwise it compiles the RAW
        // (un-preprocessed) block content.
        let profile_hash = compile_profile_hash(profile);

        // ── ONE coherent source snapshot ──
        // Every content-determined input derives from this single read
        // (identical to the HostBacked cache-miss path), so the bytes and
        // analysis cohere. The render lane consults NO cache output node and
        // runs NO classification; the override-aware reads below consume the
        // SAME stored override layers the HostBacked cache-miss path does —
        // host state, not the Stage-C session wrapper.
        let source_snap =
            self.scheduler
                .try_get_source(&canonical)
                .ok_or_else(|| HostError::MissingSource {
                    canonical_id: canonical.clone(),
                })?;
        let efs = self
            .effective_file_state_from_snapshot(&source_snap, &canonical, Some(profile_hash))
            .ok_or_else(|| HostError::MissingSource {
                canonical_id: canonical.clone(),
            })?;

        let compile_input = {
            use crate::host_executor::HostSourceData;
            let hd = source_snap
                .downcast_data::<HostSourceData>()
                .ok_or_else(|| HostError::MissingSource {
                    canonical_id: canonical.clone(),
                })?;
            let parse = &hd.parse;
            // Override-aware meta over the RAW snapshot meta — the SAME base
            // the HostBacked path feeds `effective_meta_from_base` (style-lang
            // overrides project over the raw parse meta).
            let effective_meta =
                self.effective_meta_from_base(parse.meta.clone(), &canonical, Some(profile_hash));
            // The stored request-time override layers for this profile —
            // read exactly like the HostBacked cache-miss path.
            let cc_ref = self.compile_cache().get(&canonical);
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
            drop(cc_ref);
            // The byte-load-bearing `CompileInput` — the SAME field mapping
            // the HostBacked cache-miss path builds (source, macro deps,
            // style v-bind vars from the same parse snapshot; override
            // layers from the same host state).
            let style_v_bind_vars = parse
                .style_analyses
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
                .collect();
            CompileInput {
                canonical_id: canonical.clone(),
                source: efs.source,
                meta: effective_meta,
                parse_diagnostics: parse.parse_diagnostics.clone(),
                src_blocks: parse.src_blocks.clone(),
                external_requests: parse.external_requests.clone(),
                style_override_layer,
                content_override_layer,
                macro_type_deps: efs.script_analysis.macro_type_deps.clone(),
                script_imports: efs.script_analysis.imports.clone(),
                script_macros: efs.script_analysis.macros.clone(),
                script_bindings: efs.script_analysis.bindings.clone(),
                framework_parse: efs.framework_parse,
                style_v_bind_vars,
            }
        };

        // The render-only compile: the SAME shared substrate + host-side
        // `Main` assembly as `compile_entry`, without the per-file wrapper
        // overhead, and with the imported-macro-resolution fatality softened
        // to a warning.
        self.compile_entry_runtime_render(&compile_input, profile)
    }

    /// Retrieve a compiled virtual file (script, template, style, or main bundle).
    ///
    /// On cache hit, returns immediately. On cache miss, compiles the file using
    /// the carrier registry, caches the result, and returns the requested node.
    /// In dev mode with [`CompileErrorPolicy::DevServeLastKnownGood`], falls back
    /// to the last successful compilation when the current source has errors.
    ///
    /// A thin projector over [`ensure_compile_artifacts`](Self::ensure_compile_artifacts):
    /// it parses the query to `(canonical, node_kind)`, drives the shared
    /// compile under [`CompileDemand::VirtualNode`], then projects the
    /// requested node from the served artifacts (a missing node is a typed
    /// [`HostError::MissingVirtualNode`]).
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

        let served = self.ensure_compile_artifacts(
            canonical_id.clone(),
            &query.compile_profile,
            CompileDemand::VirtualNode(node_kind.clone()),
        )?;

        let found =
            served
                .outputs
                .get(&node_kind)
                .ok_or_else(|| HostError::MissingVirtualNode {
                    canonical_id: canonical_id.clone(),
                })?;

        Ok(VirtualFileResponse {
            id: render_single_id(&canonical_id, &node_kind, &served.meta, raw_was_lsp),
            code: found.code.clone(),
            source_map: found.source_map.clone(),
            lang: found.lang.clone(),
            stale: served.stale,
            diagnostics: served.diagnostics.clone(),
            meta: found.meta.clone(),
            cache_hit: served.cache_hit,
            requested_mode: served.requested_mode,
            actual_mode: served.actual_mode,
            downgrade_reason: served.downgrade_reason,
        })
    }

    /// Drive the shared compile for `(canonical, profile)` and return the
    /// full artifact set ([`CompileServe`]): every virtual node PLUS the IDE
    /// `CachedTsx`, produced in one pass. The `demand` is consulted ONLY to
    /// gate the warm-hit consult and validate the served result — it is
    /// checked AFTER the shared compute, never steering it. `Ide` never
    /// requests a virtual node (and notably never `Main`): a carrier that
    /// projects only an IDE surface satisfies it through the served `tsx`.
    pub(crate) fn ensure_compile_artifacts(
        &self,
        canonical_id: String,
        profile: &CompileProfile,
        demand: CompileDemand,
    ) -> Result<CompileServe, HostError> {
        let profile_hash = compile_profile_hash(profile);
        let requested_mode = profile.requested_mode;

        // Cache hit check and compile input extraction under a single read lock.
        // This avoids cloning the full FileEntry (with all compile_slots, style_overrides, etc.)
        // on the hot path.
        struct CacheMiss {
            compile_input: CompileInput,
            fallback_last_good: Option<FxHashMap<VirtualNodeKind, CachedVirtualFile>>,
            meta: FileMeta,
            /// Captured from the request's single source snapshot so the
            /// compile slot is stored with the semantic_hash that was
            /// current when we decided to compile.
            semantic_hash: Hash16,
            /// The mode classification, computed once from this request's
            /// effective eligibility surface. The classifier is the sole
            /// authority for the mode decision and gates the warm-hit
            /// consult, so it must be known BEFORE any cache read. Carried
            /// out of the block so the audit event and the compile/publish
            /// routing reuse the single classification.
            classification: crate::compile_cache_mode::CompileModeClassification,
            /// `Content`-mode publish stamps, captured BEFORE the compile
            /// from the SAME source snapshot that supplies the compiled
            /// bytes: the full content-addressed key (content hash +
            /// env-hash bundle + project identity live INSIDE it) plus
            /// the project generation. The publish uses ONLY these
            /// captured values and declines (ReturnOnly) when the live
            /// identity — content hash included — has moved; a
            /// post-compile live re-read would stamp old-input bytes
            /// under a new-current identity. `None` for `Session` /
            /// `Stateless`.
            content_publish_stamp: Option<(crate::cache_runtime::CompileOutputPureContentKey, u64)>,
        }

        // The request's SINGLE scheduler source snapshot. ALL
        // content-determined inputs derive from this one coherent read:
        // the compiled bytes and script analysis (via
        // `effective_file_state_from_snapshot`), the style v-bind vars
        // (`parse.style_analyses`), the effective meta base, the
        // `Content` key's content hash, the Session slot's
        // `semantic_hash`, and the artifact-commit generation.
        // Independent re-reads could each observe a different source
        // version, pairing bytes from one version with the key hash of
        // another.
        let source_snap = self
            .scheduler
            .try_get_source(&canonical_id)
            .ok_or_else(|| HostError::MissingSource {
                canonical_id: canonical_id.clone(),
            })?;

        let cache_miss = {
            {
                use crate::host_executor::HostSourceData;

                let hd = source_snap
                    .downcast_data::<HostSourceData>()
                    .ok_or_else(|| HostError::MissingSource {
                        canonical_id: canonical_id.clone(),
                    })?;
                let parse = &hd.parse;

                // Test-only seam: the snapshot→compile-input window.
                // Fence tests land a content upsert here to prove the
                // compiled bytes and the content-addressed key cohere
                // with ONE source snapshot and the publish fence
                // detects the content movement.
                #[cfg(test)]
                {
                    let hook = self.compile_input_seam_hook.lock().clone();
                    if let Some(hook) = hook {
                        hook();
                    }
                }

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
                    .effective_file_state_from_snapshot(
                        &source_snap,
                        &canonical_id,
                        Some(profile_hash),
                    )
                    .ok_or_else(|| HostError::MissingSource {
                        canonical_id: canonical_id.clone(),
                    })?;
                // The content hash of the bytes the compile actually
                // consumes — `efs` and `parse` derive from the same
                // snapshot, and a `Content` request never carries a
                // content override (`HasBlockOverride` downgrades it
                // to `Stateless`), so for a `Content` publish this is
                // the snapshot's `whole_hash`.
                let effective_whole_hash = efs.whole_hash;
                let effective_meta = self.effective_meta_from_base(
                    parse.meta.clone(),
                    &canonical_id,
                    Some(profile_hash),
                );

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
                // Fact-validated like the warm-hit consult below: a
                // cross-file edit that invalidates the slot's recorded
                // read set suppresses the last-good fallback too, so a
                // dependency-caused compile failure surfaces instead of
                // serving the pre-edit output.
                //
                // Like the warm-hit consult, the last-good serve has NO
                // outer publish / is_stable fence, so the validator runs
                // ONLY against a proven-`Current` view: a known-stale
                // `StoreViewRead::ReturnOnly` snapshot suppresses the
                // fallback (fail-closed) rather than validating the
                // slot's cross-file `fact_versions` against
                // already-mutated dependency state. The store-view read
                // happens inside the validator closure, which
                // `peek_last_good` invokes only after its cheap
                // slot-present + carrier + non-empty-fact-rail
                // predicates pass — a slot miss or an empty fact rail
                // never builds a workspace snapshot.
                let fallback_last_good = cc_ref.as_ref().and_then(|cc| {
                    let session_node =
                        crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
                    session_node.peek_last_good(cc, profile_hash, |sig| {
                        #[cfg(test)]
                        crate::resolver_store::record_compile_warm_validation_view_read();
                        self.resolver_store_view_read()
                            .current()
                            .is_some_and(|current_view| {
                                self.compile_slot_facts_validate(&current_view, sig)
                            })
                    })
                });

                // Style v-bind vars from the SAME source snapshot the
                // compiled bytes and the cache key derive from
                // (override-independent). The analysis stage's
                // `style_analyses` is a clone of this parse field; an
                // independent analysis-snapshot read races the
                // scheduler's Source→Analysis commit window and would
                // compile — and publish warm under an unmoved key —
                // EMPTY v-bind vars.
                let style_analyses = &parse.style_analyses;

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
                    framework_parse: efs.framework_parse,
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
                // Test-only observable: the cache-mode classification the
                // RuntimeRender lane skips entirely (fixed render target, no
                // IDE-carrier cache decision). See
                // `VerterHost::wrapper_cache_mode_classification_count`.
                #[cfg(test)]
                self.test_force
                    .wrapper_cache_mode_classification_count
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let classification = crate::compile_cache_mode::classify_compile_mode(
                    requested_mode,
                    &crate::compile_cache_mode::EligibilityInputs {
                        input: &compile_input,
                        profile,
                        config: &self.config,
                        workspace_aliases: &self.workspace_aliases_for_canonical(&canonical_id),
                        owner_has_module_augmentation,
                    },
                );
                let actual_mode = classification.actual_mode;

                // `Content`-mode flight-captured publish stamps: the
                // content key (env-hash bundle + project identity) and
                // the project generation, captured HERE — before the
                // warm-hit consult and the compile — so the publish
                // never re-reads identity state the compile did not run
                // under. The same captured key drives the warm-hit peek
                // below (one key construction per request).
                let content_publish_stamp = (actual_mode == CompileCacheMode::Content).then(|| {
                    (
                        self.compile_pure_content_key(&canonical_id, effective_whole_hash, profile),
                        self.project_type_store.current_project_generation(),
                    )
                });

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
                    tsx: Option<CachedTsx>,
                }
                let warm_hit: Option<WarmHit> = match actual_mode {
                    CompileCacheMode::Stateless => None,
                    // TOP-LEVEL warm validator: a `Session` compile warm hit
                    // returns the cached compile output to the caller with NO
                    // outer publish / is_stable fence. The fact-validated
                    // session node validates the slot's cross-file
                    // `fact_versions`, so it MUST run against a proven-`Current`
                    // view. A known-stale `StoreViewRead::ReturnOnly` snapshot
                    // misses to cold (`acquire_view` yields `None`), routing the
                    // request to the recompile below whose own request driver
                    // re-fences promotion.
                    //
                    // `cc_ref` being `Some` only means a `ProfileState` exists
                    // for this canonical — the first Session compile after an
                    // upsert leaves an empty `ProfileState` with NO slot for
                    // this profile_hash. The store-view read is threaded through
                    // the `acquire_view` callback `lookup` invokes ONLY after
                    // its cheap slot-present + carrier + hash predicates pass, so
                    // that cold/profile-miss path (and a hash mismatch) never
                    // builds a full-workspace store-view snapshot.
                    CompileCacheMode::Session => cc_ref.as_ref().and_then(|cc| {
                        let session_node =
                            crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
                        session_node
                            .lookup(
                                cc,
                                profile_hash,
                                &parse.semantic_hash,
                                soh,
                                coh,
                                || {
                                    #[cfg(test)]
                                    crate::resolver_store::record_compile_warm_validation_view_read(
                                    );
                                    self.resolver_store_view_read().current()
                                },
                                |current_view, sig| {
                                    self.compile_slot_facts_validate(current_view, sig)
                                },
                            )
                            .map(|hit| WarmHit {
                                outputs: hit.outputs,
                                diagnostics: hit.diagnostics,
                                tsx: hit.tsx,
                            })
                    }),
                    CompileCacheMode::Content => {
                        let (key, _) = content_publish_stamp
                            .as_ref()
                            .expect("Content mode always captures its publish stamp");
                        self.compile_output_pure_content()
                            .peek(key)
                            .map(|value| WarmHit {
                                outputs: value.outputs.clone(),
                                diagnostics: value.diagnostics.clone(),
                                tsx: value.tsx.clone(),
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

                    // A warm hit is served only for the classified mode.
                    // A `Content` warm hit implies no reason fired — a reason
                    // would have downgraded the request to `Stateless`, which
                    // bypasses this consult — so a `Content` hit always carries
                    // `actual == requested` with no downgrade reason. A
                    // `Session` warm hit is served from the validated session
                    // node and may still carry `Some(reason)`: `Session` stays
                    // `Session` under every reason and retains the reasons for
                    // telemetry, so `downgrade_reason` can be `Some(reason)`
                    // while `actual == requested`.
                    //
                    // The DEMAND is checked AFTER the shared warm result: a
                    // warm hit serves only when it actually satisfies the
                    // demand (`VirtualNode` ⇒ the node is present;
                    // `Ide` ⇒ a `tsx` is present). An unsatisfied warm hit
                    // falls through to a cold recompute that produces the
                    // missing surface.
                    let serve = CompileServe {
                        outputs: hit.outputs,
                        tsx: hit.tsx,
                        meta: hit_meta,
                        diagnostics: hit.diagnostics,
                        stale: false,
                        cache_hit: true,
                        requested_mode,
                        actual_mode,
                        downgrade_reason: classification.first_downgrade_reason(),
                    };
                    if Self::compile_serve_satisfies_demand(&serve, &demand) {
                        return Ok(serve);
                    }
                }

                drop(cc_ref);

                CacheMiss {
                    compile_input,
                    fallback_last_good,
                    meta: effective_meta,
                    semantic_hash: parse.semantic_hash,
                    classification,
                    content_publish_stamp,
                }
            }
        };

        let CacheMiss {
            compile_input,
            fallback_last_good,
            meta,
            semantic_hash: captured_semantic_hash,
            classification,
            content_publish_stamp,
        } = cache_miss;

        #[cfg(feature = "session_metrics")]
        self.metrics
            .compile_requests
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Feature-independent cold-compile rail (see
        // `MetaProvenance::compile_cold_runs`): bumped once per cold run past
        // the warm-hit consult — the deterministic observability of compile-slot
        // COALESCING that the `session_metrics`-gated `compile_requests` mirrors.
        self.provenance
            .compile_cold_runs
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
                let force_n = self
                    .compile_force_overflow_observations
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
                self.compile_entry(&compile_input, profile)
            });
            // `Cacheable(sig)` → publish the compile-output slot through
            // the typed session node under the path-precise signature.
            // `NonCacheable` (fenced serve, overflow) → the session node
            // removes any prior slot and the freshly computed value is
            // returned without admitting. The caller-visible result is
            // computed independently of admission.
            //
            // ReturnOnly never publishes — fenced-serve arm: a compile
            // whose traced scope consumed a FENCED (ReturnOnly,
            // `store_published == false`) `IndexedReady` serve derived
            // its output from a served-without-publication artifact
            // while its fact stamps are read from the LIVE post-mutation
            // state — an entry the read-side fact rail cannot reject.
            // Consult the tracer's by-value flag and refuse admission;
            // the caller is still served the fresh output below.
            let non_cacheable_read_observed = fact_read_set.non_cacheable_read_observed();
            let admission = if non_cacheable_read_observed {
                crate::cache_runtime::SignatureAdmission::NonCacheable(
                    crate::cache_runtime::NonAdmissionReason::GenerationSuperseded,
                )
            } else {
                crate::cache_runtime::SignatureAdmission::from_finalise(fact_read_set.finalise())
            };
            (result, Some(admission))
        } else {
            // `Content` / `Stateless`: no tracer, no fact signature.
            let result = self.compile_entry(&compile_input, profile);
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

        // Test-only seam: the compute→publish window. Fence tests land
        // an env / project mutation here to prove the mode-routed
        // publish below declines instead of stamping the old-input
        // output under the moved identity.
        #[cfg(test)]
        {
            let hook = self.compile_publish_seam_hook.lock().clone();
            if let Some(hook) = hook {
                hook();
            }
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
                //
                // Flight-captured stamp discipline: the key — content
                // hash INCLUDED — and the generation were captured
                // BEFORE the compile, from the same source snapshot
                // that supplied the compiled bytes. The publish fences
                // on the LIVE identity across EVERY key dimension:
                // when the content hash, the env-hash bundle, or the
                // project identity moved in the compute→publish
                // window, the compile may have observed a torn mix of
                // the two states (the analysis-node and override-layer
                // reads are taken against the captured version), so
                // the output is attributable to NEITHER identity:
                // decline the publish (ReturnOnly — the caller is
                // still served the fresh output) and stamp nothing. A
                // vanished live source declines the same way. On an
                // unmoved identity the entry lands under the captured
                // key with the captured generation (conservatively
                // stale, never a forged-current stamp).
                let (captured_key, captured_generation) =
                    content_publish_stamp.expect("Content mode always captures its publish stamp");
                let live_key = self
                    .scheduler
                    .try_get_source(&canonical_id)
                    .and_then(|snap| {
                        snap.downcast_data::<crate::host_executor::HostSourceData>()
                            .map(|live_hd| {
                                self.compile_pure_content_key(
                                    &canonical_id,
                                    live_hd.parse.whole_hash,
                                    profile,
                                )
                            })
                    });
                if live_key.as_ref() == Some(&captured_key) {
                    self.compile_output_pure_content().publish_content(
                        captured_key,
                        compile_output_value,
                        captured_generation,
                    );
                }
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
                    // (the profileless source-derived cache) through
                    // the slot's single write authority. The admission
                    // states this lane's facts: the bytes are
                    // store-authoritative only without a content
                    // override; the stamp is the flight's captured
                    // source generation — the compile derives entirely
                    // from `source_snap`; external-src SFCs and
                    // parse-affecting profile extractions decline (the
                    // slot stores the DEFAULT extraction of the
                    // canonical's own inline bytes only).
                    if let Some(template_analysis) = compiled_template_analysis.clone() {
                        self.persist_raw_template_analysis(
                            &canonical_id,
                            Arc::new(template_analysis),
                            crate::types::RawTemplateSlotAdmission {
                                store_published: compile_input.content_override_layer.is_none(),
                                source_generation: Some(source_snap.generation),
                                has_src_blocks: !compile_input.src_blocks.is_empty(),
                                default_extraction: !profile.has_parse_affecting_template_options(),
                            },
                        );
                    }

                    // Commit to scheduler artifact snapshot (scheduler
                    // path only). Gated on `Cacheable` admission so the
                    // carrier invariant holds at the artifact substrate
                    // layer too — a refused compile must not be observable
                    // via `try_get_artifact` or pending Artifact requests.
                    self.scheduler.commit_artifact(
                        &canonical_id,
                        profile_hash,
                        verter_scheduler::node::ArtifactSnapshot {
                            generation: source_snap.generation,
                            profile_hash,
                            data: Arc::new(crate::host_executor::HostArtifactData {
                                outputs: compiled_outputs.clone(),
                                diagnostics: diagnostics.clone(),
                            }),
                        },
                    );
                } else {
                    // Refused admission. Symmetrically evict any prior
                    // scheduler artifact snapshot so `try_get_artifact`
                    // and pending Artifact requests cannot return a stale
                    // result on the companion warm-hit substrate; no fresh
                    // artifact is committed.
                    //
                    // The eviction is gated on the compile's
                    // start-of-compile generation captured on the
                    // request's single source snapshot: a slow refused
                    // compile that started at generation N can race with
                    // a fast successful compile at N+k that already
                    // committed a newer artifact, and an unconditional
                    // evict would clobber it. Passing the captured start
                    // generation as `max_generation` makes the eviction
                    // symmetric with `commit_artifact`'s own
                    // node-generation rejection.
                    self.scheduler.remove_artifact_if_not_newer_than(
                        &canonical_id,
                        profile_hash,
                        source_snap.generation,
                    );
                }
            }
        }

        // Write per-profile state to files (WASM path only).

        // The shared cold compile produced the WHOLE artifact set (every
        // virtual node + the IDE `CachedTsx`). Return it; the caller projects
        // the surface its demand requires (`get_virtual_file` projects a node;
        // `ensure_ide_compiled` checks the `tsx`). The demand is NOT consulted
        // here — a complete compute serves both surfaces.
        Ok(CompileServe {
            outputs: compiled_outputs,
            tsx: compiled_tsx,
            meta,
            diagnostics,
            stale,
            cache_hit: false,
            requested_mode: classification.requested_mode,
            actual_mode,
            downgrade_reason,
        })
    }

    /// Whether a served compile result satisfies the demand. The shared
    /// authority for the WARM-hit serve gate: a `VirtualNode` demand needs the
    /// node present; an `Ide` demand needs a `tsx`.
    fn compile_serve_satisfies_demand(serve: &CompileServe, demand: &CompileDemand) -> bool {
        match demand {
            CompileDemand::VirtualNode(kind) => serve.outputs.contains_key(kind),
            CompileDemand::Ide => serve.tsx.is_some(),
        }
    }

    /// Normalize a caller profile to one that REQUESTS the IDE/TSX surface.
    ///
    /// The IDE TSX is produced only when the compile profile's target carries
    /// the `TSX` bit (`want_ide = profile.target.needs_tsx()`). A caller's
    /// runtime profile (e.g. the bundler default, no TSX) would otherwise drive
    /// a compile that yields no `CachedTsx`, so the IDE-ensure path (and the
    /// `get_ide` peek) MUST first add the `TSX` bit. Adding it is idempotent for
    /// an already-IDE profile (the LSP `tsx_profile`), so the normalized
    /// `profile_hash` is stable across both paths: `ensure_ide_compiled`
    /// populates exactly the slot `get_ide` peeks. Every other knob
    /// (source-map, production, SSR, overrides) is preserved verbatim, so the
    /// IDE projection still reflects the caller's source-map / production
    /// choices.
    fn ide_normalized_profile(profile: &CompileProfile) -> CompileProfile {
        let mut normalized = profile.clone();
        normalized.target |= verter_compiler::compile::CompileTarget::TSX;
        normalized
    }

    /// Ensure the IDE (`CachedTsx`) projection exists for `(canonical, profile)`.
    ///
    /// Drives the shared compile under [`CompileDemand::Ide`] — it NEVER
    /// requests `VirtualNodeKind::Main`, so a carrier that projects only an IDE
    /// surface (Svelte today) succeeds without a runtime `Main`. The caller's
    /// profile is normalized to an IDE/TSX-bearing target INTERNALLY (see
    /// [`Self::ide_normalized_profile`]) so the compile produces the IDE surface
    /// regardless of the caller's runtime target. Return contract:
    ///
    /// * `Ok(true)` — `(canonical, profile)` now has a cached `CachedTsx` (an
    ///   immediate [`get_ide`](Self::get_ide) returns `Some`). This holds
    ///   WHENEVER the carrier has an IDE surface — even when the caller passed a
    ///   bundler / runtime profile with no `TSX` bit.
    /// * `Ok(false)` — the loaded file has NO IDE projection surface (e.g. a
    ///   non-carrier / a carrier that declined IDE): no error, simply nothing
    ///   to project. `Ok(false)` means a genuine no-IDE-surface, never "the
    ///   caller's profile happened to lack the TSX target".
    /// * `Err(_)` — a real failure (missing source, compile error, …); a real
    ///   failure is NEVER collapsed into `Ok(false)`.
    ///
    /// `get_ide` stays a PURE cached read — it never computes on read; this is
    /// the explicit ensure path callers invoke first.
    pub fn ensure_ide_compiled(
        &self,
        canonical_id: &str,
        profile: &CompileProfile,
    ) -> Result<bool, HostError> {
        use crate::host_executor::HostSourceData;
        let canonical = self.resolve_alias_or_canonical(canonical_id);

        // No IDE projection surface for a NON-carrier (a plain script): the
        // contract's `Ok(false)`, never a compile attempt. The carrier gate
        // mirrors `ensure_compiled`'s — every framework carrier (Vue OR
        // Svelte) projects an IDE surface; everything else does not.
        {
            let snap = self.scheduler.try_get_source(&canonical).ok_or_else(|| {
                HostError::MissingSource {
                    canonical_id: canonical.clone(),
                }
            })?;
            let hd =
                snap.downcast_data::<HostSourceData>()
                    .ok_or_else(|| HostError::MissingSource {
                        canonical_id: canonical.clone(),
                    })?;
            if !hd.file_language.is_framework_carrier() {
                return Ok(false);
            }
        }

        // A real failure (missing source, compile error) propagates as `Err`
        // — never collapsed into `Ok(false)`. A successful compile that
        // produced no IDE artifact (a non-carrier surface) is `Ok(false)`. The
        // profile is normalized to carry the `TSX` target bit so `want_ide` is
        // driven regardless of the caller's runtime target; the compile + the
        // subsequent `get_ide` read share the SAME normalized profile, so the
        // slot the `CachedTsx` lands in is exactly the one `get_ide` peeks.
        let ide_profile = Self::ide_normalized_profile(profile);
        let served = self.ensure_compile_artifacts(canonical, &ide_profile, CompileDemand::Ide)?;
        Ok(served.tsx.is_some())
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
        // Peek the IDE/TSX-normalized slot — the SAME slot `ensure_ide_compiled`
        // populates. A caller that ensured with a bundler / runtime profile (no
        // TSX bit) lands its `CachedTsx` in the TSX-normalized slot; peeking the
        // un-normalized profile_hash would miss it. Normalization is idempotent
        // for an already-IDE profile, so the LSP `tsx_profile` path is unchanged.
        let ide_profile = Self::ide_normalized_profile(profile);
        let profile_hash = compile_profile_hash(&ide_profile);

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
    /// macro-only extraction (OXC parse â†’
    /// defineProps/Emits/Slots/Model/Expose/Options)
    /// and generates a `ComponentPublicInstance`-based declaration.
    ///
    /// Returns `None` if the file is not in the host or not a Vue SFC.
    pub fn get_public_api(&self, canonical_id: &str) -> Option<TscResponse> {
        self.get_public_api_with_mode(canonical_id, PublicApiMode::Public, None)
    }

    /// Batch public-API render: ONE [`crate::resolver_store::BatchFixedView`]
    /// captured for the WHOLE batch, threaded into every item. Preserves input
    /// order; one slot per input (`None` for a non-carrier / missing canonical).
    ///
    /// Items run SEQUENTIALLY under the one fixed view (no batch-coordinator
    /// fan-out): the public-API path mutates the dependency cache + workspace
    /// edges via [`Self::sync_transitive_macro_type_dependencies`], so
    /// parallelizing it would make the dependency updates nondeterministic.
    /// Sequential + one shared view already gives O(N). Cross-item correctness is
    /// served by per-item ON-DEMAND materialization + GLOBAL artifact
    /// publication, NOT a shared batch overlay: each item's render builds its OWN
    /// fresh [`crate::resolver_core::CanonicalCompletionOverlay`] (it does NOT
    /// inherit prior items' overlays). The shared cold seed only supplies the
    /// stable base snapshot that avoids the O(N) per-call store-view rebuild; a
    /// later item importing an earlier item's type resolves it through the
    /// on-demand `ensure_indexed_ready_serve` / `ensure_loaded` path against
    /// globally-published artifacts. Default `Public` mode / no profile — the
    /// scalar surface verter-tsc consumes.
    pub fn get_public_api_batch(&self, canonical_ids: &[&str]) -> Vec<Option<TscResponse>> {
        if canonical_ids.is_empty() {
            return Vec::new();
        }
        // ONE store-view read for the whole batch (the O(N²) cliff collapse):
        // the legacy per-call `resolver_store_view_read()` on the macro-deps
        // render path is gone — every item threads this one capture's cold
        // seed. The host-level public-API path carries no session overlay, so
        // the base `HostViewRef` is the session view (an empty-overlay capture).
        //
        // CAVEAT: threading the BASE host view is correct ONLY because the
        // public-API surface has no session-scoped entry. A future
        // session-scoped public-API entry MUST thread the real overlay/session
        // view (and likely a `SessionResolverContext`) here, NOT this base view.
        let view = crate::session_view::HostViewRef::new(self);
        let fixed = self.capture_batch_fixed_view(&view);
        self.render_public_api_items(canonical_ids, PublicApiMode::Public, None, &fixed, &view)
    }

    /// The shared per-item public-API render body (scalar `N=1` + batch `N`).
    ///
    /// Each item is dispatched through the framework registry's component-API
    /// projector leg (`api_projector_for` — registry dispatch by resolved
    /// adapter id, NOT a hard Vue branch), with the batch-shared cold seed +
    /// session view threaded via the `render_seed` ctx carrier so the render
    /// takes ZERO per-call store-view reads. Scalar and batch are byte-identical
    /// by construction (both are THIS body).
    fn render_public_api_items(
        &self,
        canonical_ids: &[&str],
        mode: PublicApiMode,
        profile: Option<&CompileProfile>,
        fixed: &crate::resolver_store::BatchFixedView,
        view: &dyn crate::session_view::SessionView,
    ) -> Vec<Option<TscResponse>> {
        canonical_ids
            .iter()
            .map(|canonical_id| {
                // The classification AUTHORITY is the RUNTIME-loaded source
                // language (the explicit `UpsertRequest.file_language` the file
                // was loaded with), resolved over the ALIAS-resolved canonical.
                // A canonical whose source is not loaded, whose language has no
                // framework adapter id, or whose adapter registers no
                // api-projector leg projects no public-API surface — a `None`
                // slot (the pre-registry non-Vue behavior).
                let canonical = self.resolve_alias_or_canonical(canonical_id);
                let file_language = self.scheduler.try_get_source(&canonical).and_then(|snap| {
                    snap.downcast_data::<crate::host_executor::HostSourceData>()
                        .map(|hd| hd.file_language.clone())
                })?;
                let adapter_id = file_language.adapter_id()?;
                let projector = self.framework_registry().api_projector_for(adapter_id)?;
                projector.render_api(crate::framework::api_projector::ComponentApiProjectorCtx {
                    host: self,
                    resolved_canonical: &canonical,
                    file_language: &file_language,
                    mode,
                    profile,
                    render_seed: Some(crate::framework::api_projector::PublicApiRenderSeed {
                        cold_seed: fixed.cold_seed(),
                        session_view: view,
                    }),
                })
            })
            .collect()
    }

    /// The consumer-facing declaration companion path (`.d.<ext>.ts`) for a
    /// framework-carrier `canonical_id` — `Foo.vue` -> `Foo.d.vue.ts`,
    /// `Foo.svelte` -> `Foo.d.svelte.ts`.
    ///
    /// Resolved through the SAME framework-adapter lookup
    /// [`Self::get_public_api_with_mode`] uses: the runtime-loaded source
    /// language (`UpsertRequest.file_language`) over the alias-resolved canonical
    /// selects the adapter id, and the registered adapter's descriptor supplies
    /// the descriptor-owned `.d.<ext>.ts` naming
    /// ([`crate::framework::descriptor::FrameworkAdapterDescriptor::declaration_carrier_identity`]).
    /// `None` when the source is not loaded, its language has no framework
    /// adapter, the adapter projects no declaration carrier, or the canonical
    /// does not carry the adapter's carrier extension — the same fail-closed
    /// boundary the public-API surface uses.
    pub fn declaration_carrier_path(&self, canonical_id: &str) -> Option<String> {
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let file_language = self.scheduler.try_get_source(&canonical).and_then(|snap| {
            snap.downcast_data::<crate::host_executor::HostSourceData>()
                .map(|hd| hd.file_language.clone())
        })?;
        let adapter_id = file_language.adapter_id()?;
        let registration = self.framework_registry().get(adapter_id)?;
        registration
            .descriptor
            .declaration_carrier_identity(&canonical)
    }

    /// Generate public API output for a Vue SFC using the requested surface mode.
    ///
    /// `PublicApiMode::Public` matches the default application-facing instance shape.
    /// `PublicApiMode::Testing` exposes internal `<script setup>` bindings in a
    /// Vue Test Utils-like debug surface. `PublicApiMode::Declaration` renders the
    /// declaration-only (`.d.<ext>.ts`) public surface — a valid `.d.ts` with no
    /// runtime/value code — that a bare framework-carrier import resolves to.
    ///
    /// When `profile` is provided, script/content overrides for that compile
    /// profile are reflected in the generated API surface.
    pub fn get_public_api_with_mode(
        &self,
        canonical_id: &str,
        mode: PublicApiMode,
        profile: Option<&CompileProfile>,
    ) -> Option<TscResponse> {
        // `N=1` of the batch body. Capture ONE `BatchFixedView` and thread its
        // shared cold seed + the base session view through the shared per-item
        // render path ([`Self::render_public_api_items`], which dispatches
        // through the framework registry's component-API projector leg —
        // registry dispatch by resolved adapter id, NOT a hard Vue branch).
        // Scalar == batch BY CONSTRUCTION (both are `render_public_api_items`),
        // and the render takes ZERO per-call store-view reads. The host method
        // stays the single entry every consumer calls. The host-level
        // public-API path carries no session overlay, so the base `HostViewRef`
        // is the session view (an empty-overlay capture).
        //
        // CAVEAT (see `get_public_api_batch`): the base host view is correct
        // ONLY because there is no session-scoped public-API entry; a future
        // session-scoped entry must thread the real overlay/session view (and
        // likely a `SessionResolverContext`), not this base view.
        let view = crate::session_view::HostViewRef::new(self);
        let fixed = self.capture_batch_fixed_view(&view);
        self.render_public_api_items(
            std::slice::from_ref(&canonical_id),
            mode,
            profile,
            &fixed,
            &view,
        )
        .into_iter()
        .next()
        .flatten()
    }

    /// The Vue public-API extraction body — the EXEMPT legacy producer the
    /// `vue` component-API projector leg delegates to.
    ///
    /// Consumes deep pipeline internals (`cached_tsc_extract` /
    /// `extract_tsc_state` / `generate_tsc_from_state` /
    /// `collect_external_types_from_loaded_files` /
    /// `sync_transitive_macro_type_dependencies`) so it stays in this module.
    /// Both [`Self::get_public_api_with_mode`] and the registry's `vue`
    /// component-API projector leg
    /// ([`crate::framework::api_projectors::VueComponentApiProjector`])
    /// converge on this one body. The caller passes the ALREADY-alias-resolved
    /// canonical (the host classified it against the same resolution), so this
    /// body renders that exact target without re-resolving — classification
    /// and rendering stay coherent under concurrent alias relabels.
    pub(crate) fn render_vue_public_api_legacy(
        &self,
        resolved_canonical: &str,
        mode: PublicApiMode,
        profile: Option<&CompileProfile>,
        render_seed: Option<crate::framework::api_projector::PublicApiRenderSeed<'_>>,
    ) -> Option<TscResponse> {
        // Already alias-resolved by the caller; own it for the body's
        // existing `&canonical` / `.clone()` consumers without re-resolving.
        let canonical = resolved_canonical.to_string();
        let profile_hash = profile.map(compile_profile_hash);

        if self.is_canonical_evicted(&canonical) {
            return None;
        }

        let (source, macro_type_deps, script_imports, cached_extract, whole_hash) = {
            let efs = self.effective_file_state(&canonical, profile_hash)?;
            // Require the source to be loaded — the rest of the flow reads
            // its derived state. (Framework classification is decided once,
            // up-front, by the registry dispatch in `get_public_api_with_mode`
            // that selected this Vue projector leg; this body carries no
            // framework gate of its own.)
            self.scheduler.try_get_source(&canonical).and_then(|snap| {
                snap.downcast_data::<crate::host_executor::HostSourceData>()
                    .map(|hd| hd.file_language.clone())
            })?;
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
                efs.script_analysis.macro_type_deps.clone(),
                efs.script_analysis.imports.clone(),
                cached,
                efs.whole_hash,
            )
        };

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
            // The batch-shared cold seed + session view from the ctx carrier —
            // NO per-call store-view read on this path (the O(N²) cliff
            // collapse). Production ALWAYS threads a seed (scalar `N=1` / batch
            // both capture one fixed view up front); a macro-bearing render
            // reaching here without one is a wiring error — fail closed (return
            // `None` via `?`) rather than re-introduce a per-call
            // `resolver_store_view_read()`.
            let seed = render_seed.as_ref()?;
            let overlay =
                std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
            let host_ctx = crate::resolver_core::HostResolverContext::from_cold_seed(
                self,
                seed.cold_seed,
                overlay,
            );
            let ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
            // Session-aware collection threading the active view (NEVER `None`
            // on the render path).
            let (external_types, _, transitive) = self
                .collect_external_types_from_loaded_files_with_view(
                    ctx,
                    &canonical,
                    &macro_type_deps,
                    &script_imports,
                    profile_hash,
                    Some(seed.session_view),
                );
            (external_types, transitive)
        };
        // Unconditional: `replace_semantic_transitive(canonical, {})`
        // CLEARS the semantic axis when the set is empty (closes F15).
        self.sync_transitive_macro_type_dependencies(&canonical, &transitive_macro_type_deps);
        let tsc_mode = match mode {
            PublicApiMode::Public => verter_compiler::tsc::TscMode::Public,
            PublicApiMode::Testing => verter_compiler::tsc::TscMode::Testing,
            PublicApiMode::Declaration => verter_compiler::tsc::TscMode::Declaration,
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

        // Test-only observable: the per-file source re-clone the
        // RuntimeRender lane avoids for a simple (no external `src=`) file.
        // See `VerterHost::wrapper_source_clone_count`.
        #[cfg(test)]
        self.test_force
            .wrapper_source_clone_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
                // Cold external-type collection context (compile-prep): seed
                // from the cold-seed's inner view; nested probes fail closed
                // on a stale seed.
                // Test-only observables: the resolver store-view read and the
                // resolver-context construction the RuntimeRender lane
                // performs ONLY for a cross-file-macro file (non-empty
                // `macro_type_deps`), never for a simple file. See
                // `VerterHost::wrapper_store_view_read_count` /
                // `wrapper_resolver_ctx_construction_count`.
                #[cfg(test)]
                self.test_force
                    .wrapper_store_view_read_count
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let store_view = self.resolver_store_view_read().into_cold_seed_view();
                let overlay =
                    std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
                #[cfg(test)]
                self.test_force
                    .wrapper_resolver_ctx_construction_count
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let host_ctx = crate::resolver_core::HostResolverContext::from_cold_seed(
                    self,
                    &store_view,
                    overlay,
                );
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

        // The host-resolved Vue cross-file inputs ride opaquely on the neutral
        // options' `framework_extras` slot — Vue's eager type-surface output
        // type stays OUT of the cross-framework carrier contract. A non-Vue
        // carrier ignores the extras; Vue downcasts them.
        let vue_extras: std::sync::Arc<dyn std::any::Any + Send + Sync> = std::sync::Arc::new(
            verter_compiler::framework_common::vue_bridge::VueRuntimeCompileExtras {
                external_types,
                prop_constness_overrides: None, // populated by the cross-file optimizer
                style_v_bind_vars: snapshot.style_v_bind_vars.clone(),
            },
        );

        // The neutral runtime-compile options the carrier consults. The host
        // owns the IDE/template-data demand (from the request scope + target
        // bits) and the source-map / production / SSR profile knobs; the
        // framework-private resolved inputs ride on `framework_extras`. The
        // carrier reads only what it supports.
        let runtime_opts = RuntimeCompileOptions {
            filename: profile
                .filename
                .clone()
                .or_else(|| Some(snapshot.canonical_id.clone())),
            is_production: profile.is_production,
            source_map: profile.source_map,
            ssr: profile.ssr,
            runtime_module_name: profile.runtime_module_name.clone(),
            component_id: profile.component_id.clone(),
            force_js: profile.force_js,
            force_vapor: profile.force_vapor,
            comments: profile.comments,
            delimiters: profile.delimiters.clone(),
            custom_elements: profile.custom_elements.clone(),
            // IDE TSX is requested when the profile target carries the TSX bit.
            want_ide: profile.target.needs_tsx(),
            // Template facts are requested by the active analysis scope OR an
            // explicit TEMPLATE_DATA target bit. (The Vue runtime path always
            // requests `extract_template_data = scope.needs_template_analysis()`.)
            want_template_data: scope.needs_template_analysis()
                || profile.target.needs_template_data(),
            types_module_name: profile.types_module_name.clone(),
            embed_ambient_types: profile.embed_ambient_types,
            conditional_root_narrowing: profile.conditional_root_narrowing,
            strict_slots: profile.strict_slots,
            framework_extras: Some(vue_extras),
        };

        // Route the runtime compile through the carrier registry, selected
        // by the file's framework-neutral parse artifact. The artifact is
        // the SINGLE dispatch authority — there is no per-framework branch.
        // A canonical with no carrier artifact (e.g. a plain script that
        // reached this path) has no runtime surface to produce.
        let Some(artifact) = snapshot.framework_parse.as_ref() else {
            return Err(
                diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
                    severity: HostSeverity::Error,
                    code: "HOST_NO_CARRIER_ARTIFACT".to_string(),
                    message: format!(
                        "no framework parse artifact for '{}' — cannot route the runtime compile",
                        snapshot.canonical_id
                    ),
                    span: None,
                }])),
            );
        };
        let Some(compiler) = crate::parse::carrier_compiler_registry()
            .compiler_for_carrier_language(&artifact.adapter_id, &artifact.language_id)
        else {
            return Err(
                diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
                    severity: HostSeverity::Error,
                    code: "HOST_NO_CARRIER_COMPILER".to_string(),
                    message: format!(
                        "no carrier compiler for adapter '{}' / language '{}'",
                        artifact.adapter_id.as_str(),
                        artifact.language_id.as_str()
                    ),
                    span: None,
                }])),
            );
        };

        // The host OWNS the cached-parse validity decision: a cached
        // artifact is reused ONLY when the source was not modified by
        // external `src=` merging and the profile carries no
        // parse-affecting template options (custom delimiters / custom
        // elements). Otherwise the carrier re-parses the merged source.
        // Either way the carrier owns the typed downcast + native compile.
        let can_use_cache =
            snapshot.src_blocks.is_empty() && !profile.has_parse_affecting_template_options();
        let fresh_artifact = if can_use_cache {
            None
        } else {
            // Route the re-parse through the COUNTED chokepoint so it stays
            // visible to the `carrier_parses` dedup rail (an uncounted raw
            // `compiler.parse` is invisible to it).
            Some(crate::parse::parse_carrier_counted(
                &self.provenance,
                compiler.as_ref(),
                &merged_source,
                &verter_compiler::framework_common::ParseOptions {
                    delimiters: profile.delimiters.clone(),
                    custom_elements: profile.custom_elements.clone(),
                },
            ))
        };
        let compile_artifact = fresh_artifact.as_deref().unwrap_or(artifact);

        let compiled = match compiler.compile_bundle(
            &merged_source,
            compile_artifact,
            &runtime_opts,
            &alloc,
        ) {
            Ok(bundle) => bundle,
            Err(unsupported) => {
                let code = match unsupported {
                    CompileUnsupported::TargetMissingIde(_) => "HOST_COMPILE_TARGET_MISSING_IDE",
                    CompileUnsupported::NoIdeProjection { .. } => "HOST_COMPILE_UNSUPPORTED",
                };
                return Err(diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![
                    HostDiagnostic {
                        severity: HostSeverity::Error,
                        code: code.to_string(),
                        message: format!(
                            "carrier '{}' cannot produce a runtime bundle for '{}'",
                            artifact.adapter_id.as_str(),
                            snapshot.canonical_id
                        ),
                        span: None,
                    },
                ])));
            }
        };

        // Lift the bundle's framework-neutral diagnostics into the host
        // `DiagnosticsSnapshot` (a Svelte projector diagnostic reaches the
        // snapshot through THIS path).
        let mut compile_diags = diagnostics.clone();
        if !compiled.diagnostics.is_empty() {
            compile_diags = compile_diags.merge(DiagnosticsSnapshot::from_vec(
                compiled
                    .diagnostics
                    .iter()
                    .map(|d| HostDiagnostic {
                        severity: match d.severity {
                            RuntimeDiagnosticSeverity::Error => HostSeverity::Error,
                            RuntimeDiagnosticSeverity::Warning => HostSeverity::Warning,
                            RuntimeDiagnosticSeverity::Info => HostSeverity::Info,
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

        // The `Main` virtual node is the framework RUNTIME module. A carrier
        // that produced a runtime surface assembles it; a carrier that
        // projects ONLY an IDE surface (Svelte today) emits NO `Main` node —
        // `get_virtual_file(Main)` then reports missing until a later block
        // lands Svelte runtime generation.
        if compiled.has_runtime_surface() {
            let main_code = match &compiled.main.body_code {
                // A carrier that emits its own self-contained ESM body uses it
                // verbatim (Svelte's official-shaped output, later blocks).
                Some(body) => body.clone(),
                // Vue: the host assembles the `_sfc_main` module from the
                // neutral block fields (its virtual-file concern).
                None => assemble_vue_main_module(
                    &snapshot.canonical_id,
                    &compiled,
                    &snapshot.meta,
                    profile,
                ),
            };
            let main_lang = compiled.main.lang.clone().unwrap_or_else(|| {
                if profile.force_js {
                    "js".to_string()
                } else {
                    snapshot
                        .meta
                        .script_lang
                        .as_deref()
                        .unwrap_or("js")
                        .to_string()
                }
            });
            outputs.insert(
                VirtualNodeKind::Main,
                CachedVirtualFile {
                    code: Arc::from(main_code),
                    source_map: if compiled.main.source_map.is_empty() {
                        None
                    } else {
                        Some(Arc::from(compiled.main.source_map.clone()))
                    },
                    lang: Some(main_lang),
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
        }

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

    /// Render-only sibling of [`Self::compile_entry`]: produces the SAME
    /// `Main` bytes through the SAME shared substrate (`compile_bundle`) and
    /// the SAME host-side [`assemble_vue_main_module`], WITHOUT the per-file
    /// session-wrapper overhead. Returns the assembled `Main` code, its
    /// optional source map, and the soft (warning-severity) diagnostics of a
    /// SUCCESSFUL render.
    ///
    /// Differences from `compile_entry` (the DECIDED drop list):
    /// - (a) the source is borrowed (`&*snapshot.source`) for the common
    ///   no-external-`src=` case instead of re-cloned; the external-`src=`
    ///   merge (which inherently allocates) is unchanged.
    /// - (e) it NEVER calls `sync_transitive_macro_type_dependencies` — the
    ///   render lane is pure and READ-ONLY w.r.t. the shared
    ///   dependency/semantic-transitive axis. The axis is authoritatively
    ///   reset by the Stage-B upsert and re-populated by whichever
    ///   HostBacked/type-resolution request needs it.
    /// - (f) the external-macro-type collector runs CONDITIONALLY — exactly
    ///   the existing `macro_type_deps.is_empty()` gate — through the ONE
    ///   shared resolver, so cross-file-macro `external_types` (a codegen
    ///   input) is produced and the render output stays byte-identical.
    ///   Simple / local-macro files skip it (where the overhead lives).
    /// - the imported-macro-resolution fatality (site 2) is SOFTENED to a
    ///   warning; every other fatal site stays hard.
    fn compile_entry_runtime_render(
        &self,
        snapshot: &CompileInput,
        profile: &CompileProfile,
    ) -> Result<RenderOnlyMain, HostError> {
        let mut diagnostics = snapshot.parse_diagnostics.clone();

        // (a) DROP the source re-clone for the common case. Only the
        // external-`src=` merge (rare, and inherently allocating) builds an
        // owned String; otherwise the substrate borrows the snapshot bytes.
        let merged_source: std::borrow::Cow<'_, str> = if snapshot.src_blocks.is_empty() {
            std::borrow::Cow::Borrowed(&*snapshot.source)
        } else {
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

            // Site 1 (missing external `src=`) stays FATAL on the render lane.
            if diagnostics.has_errors {
                return Err(HostError::CompileError(CompileFailure {
                    diagnostics,
                    requested_mode: profile.requested_mode,
                    actual_mode: profile.requested_mode,
                    downgrade_reason: None,
                }));
            }

            std::borrow::Cow::Owned(merge_external_sources(
                &snapshot.source,
                &snapshot.src_blocks,
                &ext_sources,
            ))
        };

        // The compiler's own parse scratch. A local `Allocator` per render
        // call passed straight into `compile_bundle` is NOT carrier-lifecycle
        // state; it is transient parse scratch, dropped at the end of this
        // call.
        let alloc = Allocator::new();

        let profile_hash = compile_profile_hash(profile);

        // (f) NARROWED: the external-macro-type collector runs CONDITIONALLY,
        // exactly the existing `macro_type_deps.is_empty()` gate. A simple /
        // local-macro file (empty deps) substitutes the empty result and
        // skips the store-view / overlay / resolver-context construction
        // entirely (where the overhead lives). A cross-file-macro file runs
        // the collector through the ONE shared resolver so `external_types`
        // (a byte-affecting codegen input) is produced. The collector is
        // READ-ONLY: unlike `compile_entry`, this lane NEVER calls
        // `sync_transitive_macro_type_dependencies` (drop (e)), so the
        // transitive set it returns is intentionally discarded.
        let (external_types, missing_macro_type_diags) = if snapshot.macro_type_deps.is_empty() {
            (None, Vec::new())
        } else {
            let store_view = self.resolver_store_view_read().into_cold_seed_view();
            let overlay =
                std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
            let host_ctx = crate::resolver_core::HostResolverContext::from_cold_seed(
                self,
                &store_view,
                overlay,
            );
            let ctx: &dyn crate::resolver_core::resolver_context::ResolverContext = &host_ctx;
            let (external_types, missing_macro_type_diags, _transitive) = self
                .collect_external_types_from_loaded_files(
                    ctx,
                    &snapshot.canonical_id,
                    &snapshot.macro_type_deps,
                    &snapshot.script_imports,
                    Some(profile_hash),
                );
            (external_types, missing_macro_type_diags)
        };

        // Soft-macro contract — STRUCTURAL, per-diagnostic, imported-macro-
        // RESOLUTION only. On the RuntimeRender lane an imported macro type
        // that could not be RESOLVED (the dependency is absent) does NOT
        // abort: the compiler degrades the type to `Unknown` (renders as
        // `null`), so the resolution diagnostic is surfaced as a WARNING on
        // the successful output. EVERY OTHER diagnostic stays FATAL — keyed
        // on its structured code, never a whole-file flag:
        //   - collector `HOST_MISSING_MACRO_TYPE_DEP` = the softenable
        //     unresolved-import case;
        //   - collector `HOST_EXTERNAL_TYPE_DEPTH_LIMIT` /
        //     `HOST_EXTERNAL_TYPE_STEP_LIMIT` = resolution RESOURCE
        //     exhaustion (a pathological/too-deep type), which stays FATAL —
        //     it is not "the import is missing" and must not be silently
        //     erased;
        //   - compiler `XUnresolvedImportedMacroType` = the same
        //     unresolved-import case surfaced from `compile_bundle` (the
        //     compiler continues + degrades to `Unknown`), softened;
        //   - compiler `XInvalidMacroType` = a RESOLVED-but-wrong-shape type
        //     (a genuine local misuse), which stays FATAL.
        // Each collector diagnostic is routed independently, so a file with
        // one missing import AND one wrong-shape/budget failure keeps the
        // latter fatal.
        let mut soft_warnings: Vec<HostDiagnostic> = Vec::new();
        let mut fatal_collector_diags: Vec<HostDiagnostic> = Vec::new();
        for d in missing_macro_type_diags {
            if d.code == "HOST_MISSING_MACRO_TYPE_DEP" {
                soft_warnings.push(HostDiagnostic {
                    severity: HostSeverity::Warning,
                    code: d.code,
                    message: d.message,
                    span: d.span,
                });
            } else {
                fatal_collector_diags.push(d);
            }
        }
        // A collector diagnostic that is NOT the softenable unresolved-import
        // case (e.g. a resolution budget overflow) stays FATAL on the render
        // lane, exactly as on HostBacked.
        if !fatal_collector_diags.is_empty() {
            diagnostics = diagnostics.merge(DiagnosticsSnapshot::from_vec(fatal_collector_diags));
            return Err(HostError::CompileError(CompileFailure {
                diagnostics,
                requested_mode: profile.requested_mode,
                actual_mode: profile.requested_mode,
                downgrade_reason: None,
            }));
        }

        let scope = self.config.effective_scope();

        let vue_extras: std::sync::Arc<dyn std::any::Any + Send + Sync> = std::sync::Arc::new(
            verter_compiler::framework_common::vue_bridge::VueRuntimeCompileExtras {
                external_types,
                prop_constness_overrides: None,
                style_v_bind_vars: snapshot.style_v_bind_vars.clone(),
            },
        );

        // The compiler-visible runtime options — byte-identical to
        // `compile_entry`'s. `want_ide` is `profile.target.needs_tsx()`
        // (false on the bundler render profile: no TSX). `want_template_data`
        // matches `compile_entry` exactly (same scope + target derivation) so
        // template extraction — and therefore the assembled `Main` — cannot
        // drift.
        let runtime_opts = RuntimeCompileOptions {
            filename: profile
                .filename
                .clone()
                .or_else(|| Some(snapshot.canonical_id.clone())),
            is_production: profile.is_production,
            source_map: profile.source_map,
            ssr: profile.ssr,
            runtime_module_name: profile.runtime_module_name.clone(),
            component_id: profile.component_id.clone(),
            force_js: profile.force_js,
            force_vapor: profile.force_vapor,
            comments: profile.comments,
            delimiters: profile.delimiters.clone(),
            custom_elements: profile.custom_elements.clone(),
            want_ide: profile.target.needs_tsx(),
            want_template_data: scope.needs_template_analysis()
                || profile.target.needs_template_data(),
            types_module_name: profile.types_module_name.clone(),
            embed_ambient_types: profile.embed_ambient_types,
            conditional_root_narrowing: profile.conditional_root_narrowing,
            strict_slots: profile.strict_slots,
            framework_extras: Some(vue_extras),
        };

        // Route through the carrier registry (the single dispatch authority)
        // — identical to `compile_entry`. Sites 3 (no artifact) and 4 (no
        // compiler) stay FATAL.
        let Some(artifact) = snapshot.framework_parse.as_ref() else {
            return Err(HostError::CompileError(CompileFailure {
                diagnostics: diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![
                    HostDiagnostic {
                        severity: HostSeverity::Error,
                        code: "HOST_NO_CARRIER_ARTIFACT".to_string(),
                        message: format!(
                        "no framework parse artifact for '{}' — cannot route the runtime compile",
                        snapshot.canonical_id
                    ),
                        span: None,
                    },
                ])),
                requested_mode: profile.requested_mode,
                actual_mode: profile.requested_mode,
                downgrade_reason: None,
            }));
        };
        let Some(compiler) = crate::parse::carrier_compiler_registry()
            .compiler_for_carrier_language(&artifact.adapter_id, &artifact.language_id)
        else {
            return Err(HostError::CompileError(CompileFailure {
                diagnostics: diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![
                    HostDiagnostic {
                        severity: HostSeverity::Error,
                        code: "HOST_NO_CARRIER_COMPILER".to_string(),
                        message: format!(
                            "no carrier compiler for adapter '{}' / language '{}'",
                            artifact.adapter_id.as_str(),
                            artifact.language_id.as_str()
                        ),
                        span: None,
                    },
                ])),
                requested_mode: profile.requested_mode,
                actual_mode: profile.requested_mode,
                downgrade_reason: None,
            }));
        };

        // The host OWNS the cached-parse validity decision — identical to
        // `compile_entry` so the substrate sees the same parse for the same
        // bytes/options.
        let can_use_cache =
            snapshot.src_blocks.is_empty() && !profile.has_parse_affecting_template_options();
        let fresh_artifact = if can_use_cache {
            None
        } else {
            Some(crate::parse::parse_carrier_counted(
                &self.provenance,
                compiler.as_ref(),
                &merged_source,
                &verter_compiler::framework_common::ParseOptions {
                    delimiters: profile.delimiters.clone(),
                    custom_elements: profile.custom_elements.clone(),
                },
            ))
        };
        let compile_artifact = fresh_artifact.as_deref().unwrap_or(artifact);

        let compiled = match compiler.compile_bundle(
            &merged_source,
            compile_artifact,
            &runtime_opts,
            &alloc,
        ) {
            Ok(bundle) => bundle,
            // Site 5 (`CompileUnsupported`) stays FATAL.
            Err(unsupported) => {
                let code = match unsupported {
                    CompileUnsupported::TargetMissingIde(_) => "HOST_COMPILE_TARGET_MISSING_IDE",
                    CompileUnsupported::NoIdeProjection { .. } => "HOST_COMPILE_UNSUPPORTED",
                };
                return Err(HostError::CompileError(CompileFailure {
                    diagnostics: diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![
                        HostDiagnostic {
                            severity: HostSeverity::Error,
                            code: code.to_string(),
                            message: format!(
                                "carrier '{}' cannot produce a runtime bundle for '{}'",
                                artifact.adapter_id.as_str(),
                                snapshot.canonical_id
                            ),
                            span: None,
                        },
                    ])),
                    requested_mode: profile.requested_mode,
                    actual_mode: profile.requested_mode,
                    downgrade_reason: None,
                }));
            }
        };

        // Lift the bundle's framework-neutral diagnostics into the host
        // snapshot. Soft-macro contract, compiler layer: the compiler emits a
        // DISTINCT `XUnresolvedImportedMacroType` code for an imported type
        // that could not be RESOLVED (it continues and degrades the type to
        // `Unknown`). On the render lane THAT code — and only that code — is
        // downgraded to a WARNING (moved onto the soft output). Every OTHER
        // compiler diagnostic stays FATAL, decided PER-DIAGNOSTIC on its own
        // code — including `XInvalidMacroType`, which is now ONLY a
        // RESOLVED-but-wrong-shape type (a genuine local misuse). There is no
        // whole-file flag: a file with one unresolved import AND one
        // wrong-shape macro keeps the wrong-shape error fatal. HostBacked
        // never reaches this path (it aborts at the collector site first).
        let mut compile_diags = diagnostics.clone();
        let mut fatal_compiled_diags: Vec<HostDiagnostic> = Vec::new();
        for d in &compiled.diagnostics {
            let severity = match d.severity {
                RuntimeDiagnosticSeverity::Error => HostSeverity::Error,
                RuntimeDiagnosticSeverity::Warning => HostSeverity::Warning,
                RuntimeDiagnosticSeverity::Info => HostSeverity::Info,
            };
            if d.code == "XUnresolvedImportedMacroType" {
                soft_warnings.push(HostDiagnostic {
                    severity: HostSeverity::Warning,
                    code: d.code.clone(),
                    message: d.message.clone(),
                    span: d.span,
                });
            } else {
                fatal_compiled_diags.push(HostDiagnostic {
                    severity,
                    code: d.code.clone(),
                    message: d.message.clone(),
                    span: d.span,
                });
            }
        }
        if !fatal_compiled_diags.is_empty() {
            compile_diags =
                compile_diags.merge(DiagnosticsSnapshot::from_vec(fatal_compiled_diags));
        }

        // Site 6 (`compile_diags.has_errors`: syntax, CodeTransform failures,
        // any non-softened compiler error) stays FATAL.
        if compile_diags.has_errors {
            return Err(HostError::CompileError(CompileFailure {
                diagnostics: compile_diags,
                requested_mode: profile.requested_mode,
                actual_mode: profile.requested_mode,
                downgrade_reason: None,
            }));
        }

        // Assemble the `Main` runtime module host-side — the SAME
        // byte-load-bearing [`assemble_vue_main_module`] `compile_entry`
        // uses. A carrier that produced no runtime surface has no `Main`.
        if !compiled.has_runtime_surface() {
            return Err(HostError::MissingVirtualNode {
                canonical_id: snapshot.canonical_id.clone(),
            });
        }
        let main_code = match &compiled.main.body_code {
            Some(body) => body.clone(),
            None => {
                assemble_vue_main_module(&snapshot.canonical_id, &compiled, &snapshot.meta, profile)
            }
        };
        let main_source_map = if compiled.main.source_map.is_empty() {
            None
        } else {
            Some(Arc::from(compiled.main.source_map.clone()))
        };
        // The `Main` language, derived IDENTICALLY to the HostBacked
        // `Main`-node path so the bundler consumer routes sub-requests the
        // same way.
        let main_lang = compiled.main.lang.clone().unwrap_or_else(|| {
            if profile.force_js {
                "js".to_string()
            } else {
                snapshot
                    .meta
                    .script_lang
                    .as_deref()
                    .unwrap_or("js")
                    .to_string()
            }
        });

        Ok(RenderOnlyMain {
            code: Arc::from(main_code),
            source_map: main_source_map,
            lang: Some(main_lang),
            diagnostics: soft_warnings,
        })
    }
}
