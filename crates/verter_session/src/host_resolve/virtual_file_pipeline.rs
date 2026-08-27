//! `impl VerterHost` — SFC virtual-file pipeline.
//!
//! Public resolve / ensure / get / list accessors and the on-demand
//! compile path through the scheduler-backed cache.

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::instant::Instant;

use super::compile_request_build::{
    build_compile_request, derive_runtime_compile_options, request_construction_refused_diagnostics,
};
use super::vue_script_extract::template_converter_inputs;
use crate::compile::{assemble_vue_main_module, VueMainAssemblyFailure};
use crate::hash::compile_profile_hash;
use crate::id::{parse_raw_id, render_ids, render_single_id};
use crate::types::*;
use crate::CompileTarget;
use crate::VerterHost;
use oxc_allocator::Allocator;
use verter_compiler::compile::format_import_specifier;
use verter_compiler::framework_common::{
    CarrierCompileOutcome, CompileUnsupported, RuntimeDiagnosticSeverity, RuntimeTemplateBlock,
};

/// Fail-closed Main-module assembly outcome as a compile error — every
/// [`VueMainAssemblyFailure`] variant (missing/uncomposable input map,
/// invalid `__sfc__` fact, fragment-grammar refusal, composition defect,
/// or publication refusal) maps to this ONE stable host diagnostic, never
/// an unwind. `source_len` spans the whole document; the failure is not
/// one authored location.
fn assembled_map_failure_diagnostics(
    failure: VueMainAssemblyFailure,
    source_len: u32,
) -> DiagnosticsSnapshot {
    DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
        severity: HostSeverity::Error,
        code: "HOST_MAIN_MODULE_ASSEMBLY_FAILED".to_string(),
        message: failure.to_string(),
        arguments: Vec::new(),
        span: verter_span::Span::new(0, source_len),
    }])
}

fn template_compose_refusal_diagnostics(
    refusal: verter_compiler::assembly::ComposeRefusal,
    source_len: u32,
) -> DiagnosticsSnapshot {
    DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
        severity: HostSeverity::Error,
        code: "HOST_UNCOMPOSABLE_TEMPLATE_SOURCE_MAP".to_string(),
        message: format!("template virtual-file map composition failed: {refusal:?}"),
        arguments: Vec::new(),
        span: verter_span::Span::new(0, source_len),
    }])
}

/// The Template virtual file's code + map. Verbatim when the template
/// declares no runtime imports; otherwise the import preamble is composed
/// through the typed assembly engine so the map stays in sync with the
/// shifted bytes. Fails closed on `Err` — never a map-dropping fallback
/// that silently serves wrong-but-plausible code.
fn compose_template_virtual_file(
    template: RuntimeTemplateBlock,
    runtime_module_name: Option<&str>,
) -> Result<(String, Option<String>), verter_compiler::assembly::ComposeRefusal> {
    if template.imports.is_empty() {
        let source_map = (!template.source_map.is_empty()).then_some(template.source_map);
        return Ok((template.code, source_map));
    }
    let runtime = runtime_module_name.unwrap_or("vue");
    let specifiers: Vec<String> = template
        .imports
        .iter()
        .map(|name| format_import_specifier(name))
        .collect();
    let preamble = format!(
        "import {{ {} }} from \"{}\"\n",
        specifiers.join(", "),
        runtime
    );
    let existing_map = (!template.source_map.is_empty()).then_some(template.source_map.as_str());
    let composed =
        verter_compiler::assembly::prepend_preamble(&preamble, &template.code, existing_map)?;
    Ok((composed.code, existing_map.map(|_| composed.source_map)))
}

#[cfg(test)]
mod compose_template_virtual_file_tests {
    use super::*;
    use verter_compiler::framework_common::{
        RuntimeOutputDescriptor, SourceMapFidelity, TemplateRenderExport,
    };

    fn template(code: &str, source_map: &str, imports: Vec<String>) -> RuntimeTemplateBlock {
        RuntimeTemplateBlock {
            code: code.to_string(),
            source_map: source_map.to_string(),
            imports,
            ssr_imports: Vec::new(),
            render_export: TemplateRenderExport::Render,
            output_descriptor: RuntimeOutputDescriptor::generated(
                code,
                None,
                &[("test:space", "test:artifact")],
                SourceMapFidelity::Approximate,
            ),
        }
    }

    #[test]
    fn no_imports_returns_the_template_verbatim() {
        let (code, map) = compose_template_virtual_file(template("const a = 1", "", vec![]), None)
            .expect("no-import template composes trivially");
        assert_eq!(code, "const a = 1");
        assert!(map.is_none());
    }

    #[test]
    fn imports_present_prepends_the_import_line_and_shifts_the_map() {
        let map_json =
            "{\"version\":3,\"sources\":[\"Comp.vue\"],\"names\":[],\"mappings\":\"MACM\"}";
        let (code, map) = compose_template_virtual_file(
            template("const n = 1", map_json, vec!["_openBlock".to_string()]),
            None,
        )
        .expect("import template composes");
        assert_eq!(
            code, "import { openBlock as _openBlock } from \"vue\"\nconst n = 1",
            "the import preamble must precede the template's own code verbatim"
        );
        let map = map.expect("a present input map must still be present after composition");
        let decoded = verter_compiler::oxc_sourcemap::SourceMap::from_json_string(&map).unwrap();
        let token = decoded
            .get_tokens()
            .next()
            .expect("the shifted segment survives composition");
        assert_eq!(
            token.get_dst_line(),
            1,
            "the segment must move down by exactly the one-line preamble"
        );
        assert_eq!(token.get_dst_col(), 6);
        assert_eq!(
            decoded.get_source(token.get_source_id().unwrap()),
            Some("Comp.vue"),
            "the original source identity must survive — never a synthetic placeholder"
        );
    }

    #[test]
    fn custom_runtime_module_name_reaches_the_import_specifier() {
        let (code, _) = compose_template_virtual_file(
            template("const n = 1", "", vec!["_openBlock".to_string()]),
            Some("@vue/runtime-dom"),
        )
        .expect("composes");
        assert!(code.starts_with("import { openBlock as _openBlock } from \"@vue/runtime-dom\"\n"));
    }
}

pub(crate) fn vue_macro_output_matches_revision(
    output: &crate::typeinfo::vue_macro_codegen::VueMacroCodegenOutput,
    expected: verter_semantic::analysis::types::Hash16,
) -> bool {
    output.origin_whole_hash == Some(expected)
}

/// Render-only `Main` from the runtime-render lane: assembled
/// `_sfc_main` bytes, optional map, and warning diagnostics of a
/// successful render. Native-only (`host_compile`).
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct RenderOnlyMain {
    pub(crate) code: Arc<str>,
    pub(crate) source_map: Option<Arc<str>>,
    /// The `Main` module language (`"ts"` / `"js"` / `"jsx"`), derived
    /// identically to the HostBacked `get_virtual_file` `Main` node so the
    /// bundler consumer (vite sub-request routing) sees the same value.
    pub(crate) lang: Option<String>,
    pub(crate) diagnostics: Vec<HostDiagnostic>,
}

/// RAII arm/clear of per-host `compile_force_overflow_observations`.
/// Drop clears so a panic cannot leak the forced state. Per-host: does
/// not poison a concurrent compile on another host.
#[doc(hidden)]
#[cfg(any(test, feature = "test-support"))]
pub struct CompileForceOverflowGuard<'h> {
    host: &'h VerterHost,
}

#[cfg(any(test, feature = "test-support"))]
impl<'h> CompileForceOverflowGuard<'h> {
    /// Set `host`'s forced observation count to `n` and return the guard.
    pub(crate) fn arm(host: &'h VerterHost, n: usize) -> Self {
        host.compile_force_overflow_observations
            .store(n, std::sync::atomic::Ordering::Relaxed);
        Self { host }
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for CompileForceOverflowGuard<'_> {
    fn drop(&mut self) {
        self.host
            .compile_force_overflow_observations
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Zero the compile-tier prefetch counter so a following cold compute
/// is observed in isolation.
#[doc(hidden)]
#[cfg(any(test, feature = "test-support"))]
pub fn reset_compile_tier_prefetch_invocations(host: &VerterHost) {
    host.compile_tier_prefetch_invocations
        .store(0, std::sync::atomic::Ordering::Relaxed);
}

/// Current compile-tier prefetch count (relaxed). Pair with
/// [`reset_compile_tier_prefetch_invocations`].
#[doc(hidden)]
#[cfg(any(test, feature = "test-support"))]
pub fn compile_tier_prefetch_invocations(host: &VerterHost) -> usize {
    host.compile_tier_prefetch_invocations
        .load(std::sync::atomic::Ordering::Relaxed)
}

/// Content-mode cache key: content-mode hash folded with the full
/// profile. In-memory only. Different profiles must not share an entry.
fn content_mode_profile_hash(profile: &CompileProfile) -> Hash16 {
    let mut buf = Vec::with_capacity(40);
    buf.extend_from_slice(b"verter.content_mode_profile.v1:");
    buf.extend_from_slice(&CompileCacheMode::Content.stable_hash());
    buf.extend_from_slice(&compile_profile_hash(profile).to_le_bytes());
    crate::hash::hash_16(&buf)
}

#[cfg(not(target_arch = "wasm32"))]
fn validate_registered_carrier_inputs(
    _input: &CompileInput,
    profile: &CompileProfile,
) -> Result<(), HostError> {
    let grammar_matches = profile
        .delimiters
        .as_ref()
        .is_none_or(|value| value.0 == "{{" && value.1 == "}}")
        && profile.custom_elements.as_ref().is_none_or(Vec::is_empty);
    if !grammar_matches {
        return Err(HostError::GrammarMismatch(
            crate::carrier_publication_store::GrammarMismatch,
        ));
    }
    Ok(())
}

/// Compiler-crate version hash. Different versions must not share a
/// content-addressed cache entry.
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
/// Demand is checked after the shared compile. Fresh success produces
/// the products the target asked for; last-known-good serves the
/// previous compile's products. `get_virtual_file` demands a virtual
/// node; IDE-ensure demands the IDE projection without `Main`. Demand
/// does not steer compute. See
/// [`VerterHost::compile_serve_satisfies_demand`] for why a validated
/// `VirtualNode` absence is terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CompileDemand {
    /// A specific virtual node (the `get_virtual_file` projection target).
    VirtualNode(VirtualNodeKind),
    /// The IDE (`CachedTsx`) projection — satisfied iff the served result
    /// carries a `tsx`. NEVER routed through `VirtualNode(Main)`.
    Ide,
    /// The compile transaction itself — `ensure_compiled`. Satisfied by
    /// any completed compile. Demanding a product would fail a style-less
    /// SFC or a no-runtime target.
    Compiled,
}

/// Shared compile result: virtual nodes, IDE `CachedTsx`, request metadata.
///
/// Fresh success serves exactly the products the target asked for (`tsx`
/// iff the target has the `TSX` bit). A failed compile under last-known-
/// good serves `stale = true` with the previous nodes and no `tsx`.
/// Absent `tsx` is either "not asked" or "stale fallback" — `stale`
/// distinguishes them.
pub(crate) struct CompileServe {
    /// Committed products or a payload-free runtime refusal. A sum: a
    /// refused request cannot serve a sibling product under the same
    /// identity.
    pub(crate) products: ServedProducts,
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

/// Product half of a [`CompileServe`], as a sum.
///
/// A runtime-surface fail-closed serve is [`Self::RuntimeSurfaceRefused`]:
/// no virtual node, no IDE `tsx`. The reason is structural, never
/// recovered by scanning diagnostics for a framework code prefix.
#[allow(clippy::large_enum_variant)]
pub(crate) enum ServedProducts {
    Produced {
        /// Per-virtual-node-kind outputs for this `(canonical, profile)`.
        outputs: FxHashMap<VirtualNodeKind, CachedVirtualFile>,
        /// The combined IDE output, present when the compile produced one.
        tsx: Option<CachedTsx>,
    },
    RuntimeSurfaceRefused {
        diagnostic_code: Arc<str>,
        message: Arc<str>,
    },
}

impl ServedProducts {
    /// The served virtual-node outputs; `None` on a refusal (which has none).
    pub(crate) fn outputs(&self) -> Option<&FxHashMap<VirtualNodeKind, CachedVirtualFile>> {
        match self {
            Self::Produced { outputs, .. } => Some(outputs),
            Self::RuntimeSurfaceRefused { .. } => None,
        }
    }

    /// The served IDE artifact; `None` on a refusal (which has none).
    pub(crate) fn tsx(&self) -> Option<&CachedTsx> {
        match self {
            Self::Produced { tsx, .. } => tsx.as_ref(),
            Self::RuntimeSurfaceRefused { .. } => None,
        }
    }

    /// The structural refusal reason, when the request was refused.
    pub(crate) fn refusal(&self) -> Option<(&str, &str)> {
        match self {
            Self::Produced { .. } => None,
            Self::RuntimeSurfaceRefused {
                diagnostic_code,
                message,
            } => Some((diagnostic_code, message)),
        }
    }
}

/// Project a CACHED transaction onto the served surface. The refusal arm maps
/// to the refusal arm; there is no path from a refusal to a product.
fn served_products_from_cached(cached: crate::types::CompileProducts) -> ServedProducts {
    match cached {
        crate::types::CompileProducts::Produced { outputs, tsx, .. } => {
            ServedProducts::Produced { outputs, tsx }
        }
        crate::types::CompileProducts::RuntimeSurfaceRefused {
            diagnostic_code,
            message,
        } => ServedProducts::RuntimeSurfaceRefused {
            diagnostic_code,
            message,
        },
    }
}

/// Fresh cold-compile commit, before last-good is composed.
///
/// Pipeline-local sum between `compile_entry` and the cache/serve
/// sinks. Exhaustive projection: neither sink can pair a refusal with
/// a product.
#[allow(clippy::large_enum_variant)]
enum CompiledProducts {
    Produced {
        outputs: FxHashMap<VirtualNodeKind, CachedVirtualFile>,
        tsx: Option<CachedTsx>,
        template_analysis: Option<verter_semantic::analysis::template::TemplateAnalysisSnapshot>,
    },
    RuntimeSurfaceRefused {
        diagnostic_code: Arc<str>,
        message: Arc<str>,
    },
}

impl CompiledProducts {
    fn outputs(&self) -> Option<&FxHashMap<VirtualNodeKind, CachedVirtualFile>> {
        match self {
            Self::Produced { outputs, .. } => Some(outputs),
            Self::RuntimeSurfaceRefused { .. } => None,
        }
    }

    fn template_analysis(
        &self,
    ) -> Option<verter_semantic::analysis::template::TemplateAnalysisSnapshot> {
        match self {
            Self::Produced {
                template_analysis, ..
            } => template_analysis.clone(),
            Self::RuntimeSurfaceRefused { .. } => None,
        }
    }

    /// The cacheable value. `stale_last_good` is the previous compile's outputs
    /// when THIS serve fell back to them; otherwise a produced transaction
    /// remembers its own outputs as last-good. A refusal committed no output, so
    /// its arm takes neither.
    fn to_cached(
        &self,
        stale_last_good: Option<FxHashMap<VirtualNodeKind, CachedVirtualFile>>,
    ) -> crate::types::CompileProducts {
        match self {
            Self::Produced {
                outputs,
                tsx,
                template_analysis,
            } => crate::types::CompileProducts::Produced {
                outputs: outputs.clone(),
                last_good_outputs: stale_last_good.or_else(|| Some(outputs.clone())),
                tsx: tsx.clone(),
                template_analysis: template_analysis.clone(),
            },
            Self::RuntimeSurfaceRefused {
                diagnostic_code,
                message,
            } => crate::types::CompileProducts::RuntimeSurfaceRefused {
                diagnostic_code: Arc::clone(diagnostic_code),
                message: Arc::clone(message),
            },
        }
    }

    fn into_served(self) -> ServedProducts {
        match self {
            Self::Produced { outputs, tsx, .. } => ServedProducts::Produced { outputs, tsx },
            Self::RuntimeSurfaceRefused {
                diagnostic_code,
                message,
            } => ServedProducts::RuntimeSurfaceRefused {
                diagnostic_code,
                message,
            },
        }
    }
}

/// The TERMINAL outcome of one [`VerterHost::compile_entry`] transaction.
///
/// A sum: the refusal arm carries no product field, so a compile that
/// fail-closed on the runtime surface its request asked for cannot hand a
/// sibling artifact to the publish or serve paths.
#[allow(clippy::large_enum_variant)]
pub(crate) enum CompileEntryOutcome {
    Produced(CompileEntryProducts),
    RuntimeSurfaceRefused(CompileEntryRefusal),
}

/// The products of a successful compile transaction.
pub(crate) struct CompileEntryProducts {
    pub(crate) outputs: FxHashMap<VirtualNodeKind, CachedVirtualFile>,
    pub(crate) diagnostics: DiagnosticsSnapshot,
    pub(crate) tsx: Option<CachedTsx>,
    pub(crate) template_analysis:
        Option<verter_semantic::analysis::template::TemplateAnalysisSnapshot>,
    pub(crate) template_class_admission:
        crate::project_semantic_dispatch::template_class_facts::TemplateClassCacheAdmission,
}

/// A compile transaction that fail-closed on the runtime surface it was asked
/// for. Carries the structural reason and the diagnostics it reports — and NO
/// product of any kind.
pub(crate) struct CompileEntryRefusal {
    pub(crate) diagnostic_code: Arc<str>,
    pub(crate) message: Arc<str>,
    pub(crate) diagnostics: DiagnosticsSnapshot,
}

/// BY-VALUE observation of what
/// [`VerterHost::prefetch_compile_tier_observation_targets`] consumed.
///
/// The prefetch runs OUTSIDE the compile fact tracer (deliberately — load /
/// index mutations must not fold into the consumer's observed read set), so
/// its fenced-serve consumption can never reach the tracer's
/// `note_non_cacheable_read_fan_out` chokepoint on its own. The Session
/// cold-compile branch replays this value INTO its tracer scope so the
/// session-slot admission consults ONE rail (`non_cacheable_read_observed`)
/// for both the in-scope and the prefetch-consumed fenced serves.
#[derive(Debug, Clone, Copy, Default)]
struct CompileTierPrefetchObservation {
    /// Whether ANY `IndexedReady` serve the prefetch consumed was FENCED
    /// (ReturnOnly, `store_published == false`) — a payload basis the
    /// read-side fact rail cannot reject.
    fenced_serve_observed: bool,
}

impl VerterHost {
    /// Resolve a raw import identifier (bundler query string or LSP `._VERTER_.` format)
    /// to its canonical ID, virtual node kind, and rendered bundler/LSP IDs.
    ///
    /// Returns `None` if the raw ID cannot be parsed.
    pub fn resolve(&self, raw_id: &str) -> Option<ResolvedId> {
        if self.config.metrics_enabled {
            self.metrics
                .resolves
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

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

        let mut blocker_ids = std::collections::BTreeSet::new();
        let mut pending_routes = Vec::new();

        for request in blockers.external_source_requests {
            let resolved = match self.resolve_for_persistent_state(
                canonical_id,
                &request.specifier,
                verter_semantic::resolver_core::ResolutionContext {
                    phase: verter_semantic::resolver_core::ResolvePhase::CodegenBlocker,
                    kind: verter_semantic::resolver_core::ResolveRequestKind::SfcSrcAttr,
                },
            ) {
                verter_workspace::ResolutionPublication::Admitted(admitted) => {
                    let Some(resolution) = admitted.into_result() else {
                        if request.resolved_canonical_id != canonical_id {
                            blocker_ids.insert(request.resolved_canonical_id);
                        }
                        continue;
                    };
                    pending_routes.push((
                        request.specifier,
                        resolution.source_id.clone(),
                        verter_semantic::resolver_core::ResolveRequestKind::SfcSrcAttr,
                    ));
                    resolution.source_id
                }
                verter_workspace::ResolutionPublication::Refused(_) => return,
            };
            if resolved != canonical_id {
                blocker_ids.insert(resolved);
            }
        }

        for dep in blockers.macro_type_deps.iter() {
            let type_resolution = self.resolve_for_persistent_state(
                canonical_id,
                &dep.import_source,
                verter_semantic::resolver_core::ResolutionContext {
                    phase: verter_semantic::resolver_core::ResolvePhase::CodegenBlocker,
                    kind: verter_semantic::resolver_core::ResolveRequestKind::TypeImport,
                },
            );
            let resolved = match type_resolution {
                verter_workspace::ResolutionPublication::Admitted(admitted) => {
                    match admitted.into_result() {
                        Some(resolution) => {
                            pending_routes.push((
                                dep.import_source.clone(),
                                resolution.source_id.clone(),
                                verter_semantic::resolver_core::ResolveRequestKind::TypeImport,
                            ));
                            Some(resolution)
                        }
                        None => match self.resolve_for_persistent_state(
                            canonical_id,
                            &dep.import_source,
                            verter_semantic::resolver_core::ResolutionContext {
                                phase: verter_semantic::resolver_core::ResolvePhase::CodegenBlocker,
                                kind: verter_semantic::resolver_core::ResolveRequestKind::EsmImport,
                            },
                        ) {
                            verter_workspace::ResolutionPublication::Admitted(admitted) => {
                                admitted.into_result().inspect(|resolution| {
                                    pending_routes.push((
                                        dep.import_source.clone(),
                                        resolution.source_id.clone(),
                                        verter_semantic::resolver_core::ResolveRequestKind::EsmImport,
                                    ));
                                })
                            }
                            verter_workspace::ResolutionPublication::Refused(_) => return,
                        },
                    }
                }
                verter_workspace::ResolutionPublication::Refused(_) => return,
            }
            .map(|resolution| resolution.source_id);
            if let Some(resolved) = resolved.filter(|resolved| resolved != canonical_id) {
                blocker_ids.insert(resolved);
            }
        }

        for (_specifier, resolved, _kind) in pending_routes {
            self.record_resolved_dependency_edge(canonical_id, &resolved);
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
    ) -> CompileTierPrefetchObservation {
        // Test/debug-only invocation count. The cold-compute path gates
        // this prefetch to `Session`; the per-host counter lets a routing
        // test observe that gate without a fact-rail side channel.
        #[cfg(any(test, feature = "test-support"))]
        self.compile_tier_prefetch_invocations
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // Fenced-serve consumption, collected BY VALUE across every
        // `IndexedReady` serve this prefetch consumes. The prefetch runs
        // OUTSIDE the compile fact tracer (so load/index mutations are not
        // folded into the observed read set), which means a FENCED
        // (ReturnOnly, `store_published == false`) serve consumed here can
        // never reach the tracer's `note_non_cacheable_read_fan_out`
        // chokepoint — yet the compile's payload is derived from the
        // prefetch-populated state. The caller replays this flag INTO the
        // Session tracer scope so the session-slot admission declines
        // (ReturnOnly never publishes).
        let mut fenced_serve_observed = false;
        let mut note_serve =
            |serve: &Option<crate::host_manage::prepared_decl::IndexedReadyServe>| {
                if let Some(serve) = serve {
                    fenced_serve_observed |= !serve.store_published;
                }
            };

        // Owner's indexed-ready must be present so the tracer can
        // resolve owner-relative import surfaces; the owner's own
        // FileArtifactStore entry is also a producer-side dependency
        // of route observation (R26).
        note_serve(&self.ensure_indexed_ready_serve(owner_canonical));

        let mut resolved_deps = std::collections::BTreeSet::<String>::new();
        let mut pending_routes = Vec::new();

        // Macro-type deps: TypeImport first, ESM fallback. The resolved
        // canonical is registered as a dependency edge; the resolution
        // itself is memoised only by the workspace owner-edge slot.
        for dep in macro_type_deps {
            let type_resolution = self.resolve_for_persistent_state(
                owner_canonical,
                &dep.import_source,
                verter_semantic::resolver_core::ResolutionContext {
                    phase: verter_semantic::resolver_core::ResolvePhase::CodegenBlocker,
                    kind: verter_semantic::resolver_core::ResolveRequestKind::TypeImport,
                },
            );
            let resolved = match type_resolution {
                verter_workspace::ResolutionPublication::Admitted(admitted) => {
                    match admitted.into_result() {
                        Some(resolution) => {
                            Some((resolution, verter_semantic::resolver_core::ResolveRequestKind::TypeImport))
                        }
                        None => match self.resolve_for_persistent_state(
                            owner_canonical,
                            &dep.import_source,
                            verter_semantic::resolver_core::ResolutionContext {
                                phase: verter_semantic::resolver_core::ResolvePhase::CodegenBlocker,
                                kind: verter_semantic::resolver_core::ResolveRequestKind::EsmImport,
                            },
                        ) {
                            verter_workspace::ResolutionPublication::Admitted(admitted) => {
                                admitted.into_result().map(|resolution| {
                                    (resolution, verter_semantic::resolver_core::ResolveRequestKind::EsmImport)
                                })
                            }
                            verter_workspace::ResolutionPublication::Refused(_) => {
                                return CompileTierPrefetchObservation {
                                    fenced_serve_observed: true,
                                };
                            }
                        },
                    }
                }
                verter_workspace::ResolutionPublication::Refused(_) => {
                    return CompileTierPrefetchObservation {
                        fenced_serve_observed: true,
                    };
                }
            };
            if let Some((resolution, resolved_kind)) = resolved {
                pending_routes.push((
                    dep.import_source.clone(),
                    resolution.source_id.clone(),
                    resolved_kind,
                ));
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
                verter_semantic::resolver_core::ResolveRequestKind::TypeImport
            } else {
                verter_semantic::resolver_core::ResolveRequestKind::EsmImport
            };
            match self.resolve_for_persistent_state(
                owner_canonical,
                import.source.as_str(),
                verter_semantic::resolver_core::ResolutionContext {
                    phase: verter_semantic::resolver_core::ResolvePhase::CodegenBlocker,
                    kind,
                },
            ) {
                verter_workspace::ResolutionPublication::Admitted(admitted) => {
                    if let Some(resolution) = admitted.into_result() {
                        pending_routes.push((
                            import.source.clone(),
                            resolution.source_id.clone(),
                            kind,
                        ));
                        if resolution.source_id != owner_canonical {
                            resolved_deps.insert(resolution.source_id);
                        }
                    }
                }
                verter_workspace::ResolutionPublication::Refused(_) => {
                    return CompileTierPrefetchObservation {
                        fenced_serve_observed: true,
                    };
                }
            }
        }

        // External `src=` blocks. The compile-tier producer observes
        // a `FileWholeHash` of each external canonical, so each
        // external dep must reach the store before the tracer runs.
        //
        // Route-source discipline: `DerivedRawState.import_routes` holds
        // ONLY caller-supplied authoritative routes
        // (`set_import_dependencies` — e.g. an aliased `src=`
        // (`@/partials/panel.html`) only the embedder's resolver can
        // map). They serve until the caller replaces them and are never
        // overwritten here; their currency rides the workspace
        // exact-resolution facts the same push installs. Everything else
        // resolves through the one owner-edge authority below, whose
        // warm candidate is reused when its observation set is
        // unchanged.
        for request in external_requests {
            let existing_route = self
                .derived_raw_cache()
                .get(owner_canonical)
                .and_then(|d| d.import_routes.get(&request.specifier).cloned());
            let resolved = if let Some(route) = existing_route {
                // Caller-authoritative route: use its canonical for the
                // indexed-ready prefetch and leave the entry untouched.
                route
                    .resolved_canonical_id
                    .clone()
                    .or_else(|| route.effective_target().map(str::to_string))
                    .unwrap_or_else(|| request.resolved_canonical_id.clone())
            } else {
                // No caller-supplied route — resolve through the
                // `SfcSrcAttr` lane. When the workspace cannot resolve
                // the specifier, the parse-time canonical answers. The
                // answer is not memoised host-side: the workspace's own
                // owner-edge candidate slot is the one memo, and a
                // specifier that becomes resolvable repairs itself
                // because the appearance advances exactly the
                // `PathProbe` the miss observed.
                let resolved = match self.resolve_for_persistent_state(
                    owner_canonical,
                    &request.specifier,
                    verter_semantic::resolver_core::ResolutionContext {
                        phase: verter_semantic::resolver_core::ResolvePhase::CodegenBlocker,
                        kind: verter_semantic::resolver_core::ResolveRequestKind::SfcSrcAttr,
                    },
                ) {
                    verter_workspace::ResolutionPublication::Admitted(admitted) => admitted
                        .into_result()
                        .map(|resolution| resolution.source_id)
                        .unwrap_or_else(|| request.resolved_canonical_id.clone()),
                    verter_workspace::ResolutionPublication::Refused(_) => {
                        return CompileTierPrefetchObservation {
                            fenced_serve_observed: true,
                        };
                    }
                };
                if !resolved.is_empty() && resolved != owner_canonical {
                    pending_routes.push((
                        request.specifier.clone(),
                        resolved.clone(),
                        verter_semantic::resolver_core::ResolveRequestKind::SfcSrcAttr,
                    ));
                }
                resolved
            };
            if !resolved.is_empty() && resolved != owner_canonical {
                resolved_deps.insert(resolved);
            }
        }

        for (_specifier, resolved, _kind) in pending_routes {
            self.record_resolved_dependency_edge(owner_canonical, &resolved);
        }

        // Drive each resolved dep to IndexedReady so its
        // `FileArtifactStore` entry (including the `facts` registry)
        // is published before the tracer queries fact hashes. Calls
        // are idempotent / cache-hit on warm reads.
        for dep_canonical in resolved_deps {
            note_serve(&self.ensure_indexed_ready_serve(&dep_canonical));
        }

        CompileTierPrefetchObservation {
            fenced_serve_observed,
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
                // through to recompile WITHOUT requesting a store-view root
                // capture.
                if let Some(cc) = self.compile_cache().get(&canonical) {
                    let coh = 0;
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
                    if let Some(hit) = session_node.lookup(
                        &cc,
                        profile_hash,
                        &hd.parse.semantic_hash,
                        coh,
                        profile.svelte_css_hash_override.as_deref(),
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
                    ) {
                        // The warm answer is the CACHED TERMINAL ARM, not the
                        // mere existence of a validated slot. A cached refusal
                        // is the final answer for this identity, so it must be
                        // reported here exactly as the cold path reports it —
                        // otherwise the same request succeeds warm and fails
                        // cold.
                        return match hit.products.refusal() {
                            Some((diagnostic_code, message)) => {
                                Err(HostError::RuntimeSurfaceRefused {
                                    canonical_id: canonical.clone(),
                                    diagnostic_code: diagnostic_code.to_string(),
                                    message: message.to_string(),
                                })
                            }
                            None => Ok(()),
                        };
                    }
                }
            }
        }

        self.hydrate_compile_blockers(&canonical);

        // Cache miss — drive the shared compile. The demand is
        // `CompileDemand::Compiled`: this route asks whether the transaction
        // for `(canonical, profile)` has run and been cached, NOT whether some
        // particular product exists. Demanding `Main` here would report a
        // missing runtime module to an identity whose target never asked for
        // one, and would disagree with the warm path above.
        let served =
            self.ensure_compile_artifacts(canonical.clone(), profile, CompileDemand::Compiled)?;
        // Same projection as the warm arm, so both answer identically.
        if let Some((diagnostic_code, message)) = served.products.refusal() {
            return Err(HostError::RuntimeSurfaceRefused {
                canonical_id: canonical,
                diagnostic_code: diagnostic_code.to_string(),
                message: message.to_string(),
            });
        }
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
        view.validates_fact_signature(&signature.facts)
    }

    /// Read-only predicate: would `get_virtual_file(query)` for this
    /// `(canonical_id, profile)` hit the compile cache without doing any
    /// work?
    ///
    /// Mirrors the freshness predicate the writer uses inside
    /// `get_virtual_file` (`slot.semantic_hash == parse.semantic_hash
    /// && slot.content_override_hash == coh && fact-signature validates`).
    /// The predicate stays in
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
        let coh = 0;
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
                coh,
                profile.svelte_css_hash_override.as_deref(),
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
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn render_only_main(
        &self,
        canonical_id: &str,
        profile: &CompileProfile,
    ) -> Result<RenderOnlyMain, HostError> {
        let canonical = self.resolve_alias_or_canonical(canonical_id);

        // External block bytes are VFS-owned compiler inputs. Resolve and
        // load those blockers before taking the coherent owner/content
        // capture below; `selected_block` is intentionally a read-only
        // selector and must not perform workspace or scheduler mutation.
        self.hydrate_compile_blockers(&canonical);

        // The SAME profile hash `apply_block_overrides` / `get_virtual_file`
        // key validated supplied block content under. The bundler's
        // preprocessor path (Pug / CoffeeScript templates+scripts, custom
        // blocks, non-Vite styles) admits supplied artifacts for this profile
        // immediately before rendering, so the render lane must read the
        // override-aware effective state — otherwise it compiles the RAW
        // (un-preprocessed) block content.
        let profile_hash = compile_profile_hash(profile);
        let block_content_capture_fence = self.block_content.admission_fence.lock();

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
            // The byte-load-bearing `CompileInput` — the SAME field mapping
            // the HostBacked cache-miss path builds (source, macro deps,
            // style v-bind vars from the same parse snapshot; override
            // layers from the same host state).
            // The SOUND OXC-derived roots recorded on each analyzed v-bind —
            // never a text split of the expression.
            let style_content = self.capture_compiler_style_content_for_profile(
                &canonical,
                &parse.style_analyses,
                profile,
            );
            let style_v_bind_vars = style_content.v_bind_vars;
            let block_content = self.capture_compiler_block_content(&canonical, profile)?;
            CompileInput {
                canonical_id: canonical.clone(),
                source: efs.source,
                whole_hash: efs.whole_hash,
                meta: effective_meta,
                parse_diagnostics: parse.parse_diagnostics.clone(),
                src_blocks: parse.src_blocks.clone(),
                external_requests: parse.external_requests.clone(),
                has_supplied_block_content: block_content.has_supplied,
                block_content_inputs: block_content.inputs,
                macro_type_deps: efs.script_analysis.macro_type_deps.clone(),
                script_imports: efs.script_analysis.imports.clone(),
                script_macros: efs.script_analysis.macros.clone(),
                script_bindings: efs.script_analysis.bindings.clone(),
                script_macro_usage: efs.script_analysis.macro_usage.clone(),
                script_vue_api_calls: efs.script_analysis.vue_api_calls.clone(),
                framework_parse: efs.framework_parse,
                style_v_bind_vars,
                style_v_bind_usage_complete: style_content.usage_complete,
            }
        };
        drop(block_content_capture_fence);

        validate_registered_carrier_inputs(&compile_input, profile)?;

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
        if self.config.metrics_enabled {
            self.metrics
                .virtual_loads
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

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

        // A transaction that fail-closed on the runtime surface it was asked for
        // committed NO product, so there is nothing to project. A `Main` request
        // — the runtime product itself — receives the EXPLICIT typed refusal,
        // carrying the carrier's own code and message read STRUCTURALLY off the
        // served refusal (never sniffed out of the diagnostics by code prefix).
        // Every OTHER node kind stays the generic `MissingVirtualNode`, so a
        // caller asking for a node this carrier never emits is not told a
        // runtime surface was refused.
        if let Some((diagnostic_code, message)) = served.products.refusal() {
            if node_kind == VirtualNodeKind::Main {
                return Err(HostError::RuntimeSurfaceRefused {
                    canonical_id: canonical_id.clone(),
                    diagnostic_code: diagnostic_code.to_string(),
                    message: message.to_string(),
                });
            }
            return Err(HostError::MissingVirtualNode {
                canonical_id: canonical_id.clone(),
            });
        }

        let found = match served.products.outputs().and_then(|o| o.get(&node_kind)) {
            Some(found) => found,
            None => {
                return Err(HostError::MissingVirtualNode {
                    canonical_id: canonical_id.clone(),
                });
            }
        };

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

        // `get_virtual_file` is a first-class compile entry, not merely the
        // projection half of `ensure_compiled`. Hydrate resolved external
        // block canonicals here so every entry observes VFS-owned bytes
        // before the single coherent compiler block-content capture. This
        // stays outside the admission fence because loading publishes source
        // state; the capture and its post-compile currentness check provide
        // the atomicity boundary.
        self.hydrate_compile_blockers(&canonical_id);
        let block_content_capture_fence = self.block_content.admission_fence.lock();

        // Cache hit check and compile input extraction under a single read lock.
        // This avoids cloning the full file entry and its compile slots.
        // on the hot path.
        struct CacheMiss {
            compile_input: CompileInput,
            fallback_last_good: Option<FxHashMap<VirtualNodeKind, CachedVirtualFile>>,
            meta: FileMeta,
            /// Captured from the request's single source snapshot so the
            /// compile slot is stored with the semantic_hash that was
            /// current when we decided to compile.
            semantic_hash: Hash16,
            /// The full-content identity of that same snapshot. The
            /// `latest_diagnostics` write is fenced on it: diagnostics
            /// describe bytes, so any content movement — a style-only edit
            /// included — makes this compile's set stale for the live
            /// buffer.
            whole_hash: Hash16,
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
            /// Exact compiler block-content projection captured with the owner
            /// snapshot. Publish revalidates this after the cold compute so a
            /// concurrent supplied apply or external/owner publication cannot
            /// re-admit stale output after clearing the live slots.
            block_content_stamp: BlockContentHashToken,
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

                let grammar_matches = profile
                    .delimiters
                    .as_ref()
                    .is_none_or(|value| value.0 == "{{" && value.1 == "}}")
                    && profile.custom_elements.as_ref().is_none_or(Vec::is_empty);
                if !grammar_matches {
                    return Err(HostError::GrammarMismatch(
                        crate::carrier_publication_store::GrammarMismatch,
                    ));
                }

                // Test-only seam: the snapshot→compile-input window.
                // Fence tests land a content upsert here to prove the
                // compiled bytes and the content-addressed key cohere
                // with ONE source snapshot and the publish fence
                // detects the content movement.
                let cc_ref = self.compile_cache().get(&canonical_id);

                // Cache hit check from compile_cache
                let coh = 0;

                // Build this request's effective compile input (override-
                // aware) and classify the cache mode BEFORE any warm-hit
                // consult. The classifier is the sole authority for the
                // mode decision and it gates the cache read: a request that
                // classifies to `Stateless` must not consult any host cache
                // node, and a `Content` warm hit is valid only when the
                // request actually classifies to `Content`. A request-time
                // supplied block content removes the session slot but does
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
                let style_content = self.capture_compiler_style_content_for_profile(
                    &canonical_id,
                    &parse.style_analyses,
                    profile,
                );
                let block_content = self.capture_compiler_block_content(&canonical_id, profile)?;

                let compile_input = CompileInput {
                    canonical_id: canonical_id.clone(),
                    source: efs.source,
                    whole_hash: effective_whole_hash,
                    meta: effective_meta.clone(),
                    parse_diagnostics: parse.parse_diagnostics.clone(),
                    src_blocks: parse.src_blocks.clone(),
                    external_requests: parse.external_requests.clone(),
                    has_supplied_block_content: block_content.has_supplied,
                    block_content_inputs: block_content.inputs,
                    macro_type_deps: efs.script_analysis.macro_type_deps.clone(),
                    script_imports: efs.script_analysis.imports.clone(),
                    script_macros: efs.script_analysis.macros.clone(),
                    script_bindings: efs.script_analysis.bindings.clone(),
                    script_macro_usage: efs.script_analysis.macro_usage.clone(),
                    script_vue_api_calls: efs.script_analysis.vue_api_calls.clone(),
                    framework_parse: efs.framework_parse,
                    // Compiler-only SOUND roots remain available even when
                    // source-located facts fail closed for another space.
                    style_v_bind_vars: style_content.v_bind_vars,
                    style_v_bind_usage_complete: style_content.usage_complete,
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
                    products: crate::types::CompileProducts,
                    diagnostics: DiagnosticsSnapshot,
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
                    // requests a store-view root capture.
                    CompileCacheMode::Session => cc_ref.as_ref().and_then(|cc| {
                        let session_node =
                            crate::cache_runtime::CompileOutputNodeFactValidatedSession::new();
                        session_node
                            .lookup(
                                cc,
                                profile_hash,
                                &parse.semantic_hash,
                                coh,
                                profile.svelte_css_hash_override.as_deref(),
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
                                products: hit.products,
                                diagnostics: hit.diagnostics,
                            })
                    }),
                    CompileCacheMode::Content => {
                        let (key, _) = content_publish_stamp
                            .as_ref()
                            .expect("Content mode always captures its publish stamp");
                        self.compile_output_pure_content()
                            .peek(key)
                            .map(|value| WarmHit {
                                products: value.products.clone(),
                                diagnostics: value.diagnostics.clone(),
                            })
                    }
                };

                if let Some(hit) = warm_hit {
                    if self.config.metrics_enabled {
                        self.metrics
                            .compile_cache_hits
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }

                    // Build effective meta for cache-hit render_ids.
                    let hit_meta = parse.meta.clone();

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
                        products: served_products_from_cached(hit.products),
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
                    whole_hash: parse.whole_hash,
                    classification,
                    content_publish_stamp,
                    block_content_stamp: block_content.stamp,
                }
            }
        };
        drop(block_content_capture_fence);

        // Test-only seam after the coherent owner + block-content capture and
        // before cold compute. A source/content mutation landed here must make
        // the post-compute publication stamp decline.
        #[cfg(test)]
        {
            let hook = self.compile_input_seam_hook.lock().clone();
            if let Some(hook) = hook {
                hook();
            }
        }

        let CacheMiss {
            compile_input,
            fallback_last_good,
            meta,
            semantic_hash: captured_semantic_hash,
            whole_hash: captured_whole_hash,
            classification,
            content_publish_stamp,
            block_content_stamp: captured_block_content_stamp,
        } = cache_miss;

        if self.config.metrics_enabled {
            self.metrics
                .compile_requests
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        // Feature-independent cold-compile rail (see
        // `MetaProvenance::compile_cold_runs`): bumped once per cold run past
        // the warm-hit consult — the deterministic observability of compile-slot
        // COALESCING that the metrics-gated `compile_requests` mirrors.
        self.provenance
            .compile_cold_runs
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let compile_start = self.config.metrics_enabled.then(Instant::now);

        let content_override_hash = 0;

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
        let prefetch_observation = if actual_mode == CompileCacheMode::Session {
            self.prefetch_compile_tier_observation_targets(
                &canonical_id,
                &compile_input.script_imports,
                &compile_input.macro_type_deps,
                &compile_input.external_requests,
            )
        } else {
            CompileTierPrefetchObservation::default()
        };

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
            let (result, fact_read_set) =
                self.with_fact_tracer(verter_workspace::AggregateBasisSeed::Unvouched, || {
                    // Replay the prefetch's BY-VALUE fenced-serve consumption
                    // into THIS tracer scope: the compile's payload derives from
                    // the prefetch-populated state, so a fenced serve consumed
                    // there taints this compile exactly as an in-scope fenced
                    // serve would — one admission rail
                    // (`non_cacheable_read_observed`), consulted below.
                    if prefetch_observation.fenced_serve_observed {
                        crate::resolver_core::resolver_context::note_non_cacheable_read_fan_out(
                        crate::resolver_core::resolver_context::NonCacheableReadReason::FencedServe,
                    );
                    }
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
        // The committed products of this transaction, plus the diagnostics it
        // reports. A refusal contributes NO product half at all — its arm never
        // constructs `outputs` / `tsx` / template analysis — which is exactly
        // why "refused AND published" cannot be expressed downstream.
        let (
            compiled_products,
            diagnostics,
            stale,
            template_class_admission,
        ) = match compile_result {
            Ok(CompileEntryOutcome::Produced(produced)) => (
                CompiledProducts::Produced {
                    outputs: produced.outputs,
                    tsx: produced.tsx,
                    template_analysis: produced.template_analysis,
                },
                produced.diagnostics,
                false,
                produced.template_class_admission,
            ),
            Ok(CompileEntryOutcome::RuntimeSurfaceRefused(refusal)) => (
                CompiledProducts::RuntimeSurfaceRefused {
                    diagnostic_code: refusal.diagnostic_code,
                    message: refusal.message,
                },
                refusal.diagnostics,
                false,
                crate::project_semantic_dispatch::template_class_facts::TemplateClassCacheAdmission::not_applicable(),
            ),
            Err(diagnostics) => {
                let publication_fence = self.block_content.admission_fence.lock();
                if self.compiler_block_content_capture_is_current(
                    &canonical_id,
                    profile,
                    captured_whole_hash,
                    &captured_block_content_stamp,
                ) {
                    self.store_latest_diagnostics_if_source_unmoved(
                        &canonical_id,
                        profile_hash,
                        captured_whole_hash,
                        diagnostics.clone(),
                    );
                }
                drop(publication_fence);
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
                        // A last-good serve is a PRODUCED outcome carrying the
                        // previous compile's outputs — never a runtime refusal.
                        (
                            CompiledProducts::Produced {
                                outputs: last_good,
                                tsx: None,
                                template_analysis: None,
                            },
                            diagnostics,
                            true,
                            crate::project_semantic_dispatch::template_class_facts::TemplateClassCacheAdmission::refused(),
                        )
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

        if let Some(compile_start) = compile_start {
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
        // The last-good rail belongs to the PRODUCED arm only: a refusal committed
        // no output, so there is nothing to remember as last-good, and the
        // conversion cannot smuggle one in.
        let compile_output_value = crate::cache_runtime::CompileOutputValue::from_compile_record(
            captured_semantic_hash,
            content_override_hash,
            profile.svelte_css_hash_override.as_deref().map(Arc::from),
            compiled_products.to_cached(if stale {
                fallback_last_good.clone()
            } else {
                None
            }),
            diagnostics.clone(),
        );

        // The `latest_diagnostics` + generation bump runs for EVERY mode
        // so compile errors / warnings surface regardless of caching.
        // This is observable diagnostic state, not a compile-output
        // cache entry — which is exactly why it is fenced on the source
        // identity these diagnostics were computed from rather than
        // written blind (see the writer's contract).
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

        let block_content_publication_fence = self.block_content.admission_fence.lock();
        let compile_capture_is_current = self.compiler_block_content_capture_is_current(
            &canonical_id,
            profile,
            captured_whole_hash,
            &captured_block_content_stamp,
        );
        if compile_capture_is_current {
            self.store_latest_diagnostics_if_source_unmoved(
                &canonical_id,
                profile_hash,
                captured_whole_hash,
                diagnostics.clone(),
            );

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
                    let (captured_key, captured_generation) = content_publish_stamp
                        .expect("Content mode always captures its publish stamp");
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
                    if live_key.as_ref() == Some(&captured_key)
                        && template_class_admission.owner_only
                    {
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
                    let admission = compile_admission
                        .expect("Session mode always finalises a SignatureAdmission");
                    let is_cacheable = matches!(
                        admission,
                        crate::cache_runtime::SignatureAdmission::Cacheable(_)
                    );
                    // The scheduler artifact carries PRODUCTS. A refused
                    // transaction has none, and an artifact holding its empty
                    // output map would re-encode the refusal as an untyped
                    // successful empty compile on that substrate — the one thing
                    // the typed terminal result exists to prevent. So a refusal
                    // takes the SAME no-publish branch an overflowed admission
                    // already takes below: commit nothing, evict any prior.
                    //
                    // Nothing depends on artifact presence for completion here.
                    // `commit_artifact` also signals + terminalizes pending
                    // Artifact DAG identities, but this crate never submits a
                    // `TaskKind::Artifact`, and the no-publish branch is already
                    // a reachable terminal outcome of this exact site.
                    let commits_artifact = is_cacheable
                        && matches!(compiled_products, CompiledProducts::Produced { .. });
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
                        if let Some(template_analysis) = compiled_products.template_analysis() {
                            self.persist_raw_template_analysis(
                                &canonical_id,
                                Arc::new(template_analysis),
                                crate::types::RawTemplateSlotAdmission {
                                    store_published: true,
                                    source_generation: Some(source_snap.generation),
                                    has_src_blocks: !compile_input.src_blocks.is_empty(),
                                    default_extraction: !profile
                                        .has_parse_affecting_template_options(),
                                    template_class_signature: template_class_admission
                                        .signature
                                        .clone(),
                                },
                            );
                        }
                    }

                    // The artifact substrate carries PRODUCTS, so the commit is
                    // sourced from the PRODUCED arm's own outputs — there is no
                    // path here that turns an absent product set into an empty
                    // committed map. A refused-admission compile and a refused
                    // runtime surface both fall to the same eviction arm.
                    match compiled_products.outputs().filter(|_| commits_artifact) {
                        Some(outputs) => {
                            self.scheduler.commit_artifact(
                                &canonical_id,
                                profile_hash,
                                verter_scheduler::node::ArtifactSnapshot {
                                    generation: source_snap.generation,
                                    profile_hash,
                                    data: Arc::new(crate::host_executor::HostArtifactData {
                                        outputs: outputs.clone(),
                                        diagnostics: diagnostics.clone(),
                                    }),
                                },
                            );
                        }
                        None => {
                            // No product to commit — either the admission was
                            // refused (overflow) or the transaction refused its
                            // runtime surface. Symmetrically evict any prior
                            // scheduler artifact snapshot so `try_get_artifact`
                            // and pending Artifact requests cannot return a
                            // stale result on the companion warm-hit substrate;
                            // no fresh artifact is committed.
                            //
                            // The eviction is gated on the compile's
                            // start-of-compile generation captured on the
                            // request's single source snapshot: a slow compile
                            // that started at generation N can race with a fast
                            // successful compile at N+k that already committed a
                            // newer artifact, and an unconditional evict would
                            // clobber it. Passing the captured start generation
                            // as `max_generation` makes the eviction symmetric
                            // with `commit_artifact`'s own node-generation
                            // rejection.
                            self.scheduler.remove_artifact_if_not_newer_than(
                                &canonical_id,
                                profile_hash,
                                source_snap.generation,
                            );
                        }
                    }
                }
            }
        }
        drop(block_content_publication_fence);

        // Write per-profile state to files (WASM path only).

        // Return the serve; the caller projects the surface its demand requires
        // (`get_virtual_file` projects a node; `ensure_ide_compiled` checks the
        // `tsx`). The demand is NOT consulted here: on a fresh SUCCESSFUL
        // compile the compute already produced every product this identity's
        // target named, so re-checking one could only reject a complete result.
        // A serve that fell back to dev last-known-good (`stale`) is the
        // exception — it carries the previous compile's virtual nodes and no
        // `tsx`, so an IDE demand against it is unsatisfied and recomputes on
        // the next request (see `compile_serve_satisfies_demand`).
        Ok(CompileServe {
            products: compiled_products.into_served(),
            meta,
            diagnostics,
            stale,
            cache_hit: false,
            requested_mode: classification.requested_mode,
            actual_mode,
            downgrade_reason,
        })
    }

    /// Whether a served compile satisfies the demand — the warm-hit serve
    /// gate.
    ///
    /// A validated serve is terminal for a `VirtualNode` demand whether
    /// or not the node is present. A successful compile is deterministic
    /// for `(canonical, profile)` once own-content hashes and the
    /// cross-file fact signature validate, so recompiling cannot create
    /// a missing node. Last-known-good is the exception (stale failed
    /// outputs); its runtime nodes are still served, not re-demanded.
    /// Target-scoped absence is the correct terminal answer: treating
    /// it as incomplete would recompile forever and still return
    /// `MissingVirtualNode`.
    ///
    /// A runtime refusal satisfies every demand: payload-free final
    /// answer for those inputs.
    ///
    /// `Ide` still requires its product: last-known-good publishes
    /// `tsx: None` under a TSX-bearing profile, and recomputing is the
    /// retry that lets a transient failure heal.
    fn compile_serve_satisfies_demand(serve: &CompileServe, demand: &CompileDemand) -> bool {
        match &serve.products {
            ServedProducts::RuntimeSurfaceRefused { .. } => true,
            ServedProducts::Produced { tsx, .. } => match demand {
                CompileDemand::VirtualNode(_) | CompileDemand::Compiled => true,
                CompileDemand::Ide => tsx.is_some(),
            },
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
        normalized.target |= crate::CompileTarget::TSX;
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
    ///   caller's profile happened to lack the TSX target", and never a
    ///   refusal.
    /// * `Err(HostError::RuntimeSurfaceRefused)` — the caller's profile ALSO
    ///   asked for a runtime product and the carrier fail-closed on it. The
    ///   transaction committed nothing, so there is no IDE artifact to report:
    ///   that is a real failure of this request, NOT a "no IDE surface" answer,
    ///   and it is never collapsed into `Ok(false)`. A profile that asks for the
    ///   IDE product alone cannot reach this arm.
    /// * `Err(_)` — any other real failure (missing source, compile error, …); a
    ///   real failure is NEVER collapsed into `Ok(false)`.
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
        let served =
            self.ensure_compile_artifacts(canonical.clone(), &ide_profile, CompileDemand::Ide)?;
        if let Some((diagnostic_code, message)) = served.products.refusal() {
            return Err(HostError::RuntimeSurfaceRefused {
                canonical_id: canonical,
                diagnostic_code: diagnostic_code.to_string(),
                message: message.to_string(),
            });
        }
        Ok(served.products.tsx().is_some())
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
    /// Returns `Ok(None)` if the file is not in the host or exposes no
    /// framework public-API carrier. Projection failures remain typed errors.
    pub fn get_public_api(
        &self,
        canonical_id: &str,
    ) -> Result<Option<TscResponse>, crate::PublicApiProjectionError> {
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
    pub fn get_public_api_batch(
        &self,
        canonical_ids: &[&str],
    ) -> Vec<Result<Option<TscResponse>, crate::PublicApiProjectionError>> {
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
    ///
    /// RESPONSE-ONLY by design: the render is adapter declaration output and
    /// never composes the structured component contract. The contract is
    /// projection-entry-scoped — [`Self::get_public_api_projection`] composes
    /// it at the demand that consumes it, under the same `(fixed, view)`
    /// capture as its render. Response-only consumers (carrier sync,
    /// background drains, batch, MCP, NAPI) therefore never pay a
    /// composed-then-discarded component-meta walk, and a completion-time
    /// reconcile never cold-loads a child through it.
    fn render_public_api_items(
        &self,
        canonical_ids: &[&str],
        mode: PublicApiMode,
        profile: Option<&CompileProfile>,
        fixed: &crate::resolver_store::BatchFixedView,
        view: &dyn crate::session_view::SessionView,
    ) -> Vec<Result<Option<TscResponse>, crate::PublicApiProjectionError>> {
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
                let Some(file_language) =
                    self.scheduler.try_get_source(&canonical).and_then(|snap| {
                        snap.downcast_data::<crate::host_executor::HostSourceData>()
                            .map(|hd| hd.file_language.clone())
                    })
                else {
                    return Ok(None);
                };
                let Some(adapter_id) = file_language.adapter_id() else {
                    return Ok(None);
                };
                let Some(projector) = self.framework_registry().api_projector_for(adapter_id)
                else {
                    return Ok(None);
                };
                projector.render_api(crate::framework::api_projector::ComponentApiProjectorCtx {
                    host: self,
                    resolved_canonical: &canonical,
                    file_language: &file_language,
                    mode,
                    profile,
                    render_seed: Some(crate::framework::api_projector::PublicApiRenderSeed {
                        cold_seed: fixed.cold_seed(),
                        view,
                        fixed,
                    }),
                })
            })
            .collect()
    }

    /// Compose one canonical's mandatory structured contract under the SAME
    /// `(fixed, view)` capture its declaration render used — the
    /// projection-entry half of the "one coherent projector invocation"
    /// contract. Composition never gates the response: an absent or failed
    /// component-meta output degrades to the typed `Unsupported` availability.
    fn compose_component_contract(
        &self,
        canonical: &str,
        adapter_id: &verter_language::FrameworkAdapterId,
        view: &dyn crate::session_view::SessionView,
        fixed: &crate::resolver_store::BatchFixedView,
    ) -> crate::framework::ComponentContractAvailability {
        match self.get_component_meta_output_via_view_with_fixed_store_view(
            canonical, view, fixed, false,
        ) {
            Ok(Some(output)) => output.contract().clone(),
            Ok(None) => crate::framework::ComponentContractAvailability::Unsupported(
                crate::framework::ComponentContractUnsupported {
                    adapter_id: adapter_id.clone(),
                    reason:
                        crate::framework::ComponentContractUnsupportedReason::ComponentMetaUnavailable,
                    diagnostics: std::sync::Arc::from([]),
                },
            ),
            Err(error) => crate::framework::public_contract::unsupported_from_output_error(
                adapter_id.clone(),
                &error,
            ),
        }
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
    ) -> Result<Option<TscResponse>, crate::PublicApiProjectionError> {
        // `N=1` of the batch body. Capture ONE `BatchFixedView` and thread its
        // shared cold seed + the base session view through the shared per-item
        // render path ([`Self::render_public_api_items`], which dispatches
        // through the framework registry's component-API projector leg —
        // registry dispatch by resolved adapter id, NOT a hard Vue branch).
        // Scalar == batch BY CONSTRUCTION (both are `render_public_api_items`),
        // and the render takes ZERO per-call store-view reads. The host method
        // stays the single entry every consumer calls. The host-level
        // A profile-owned content override is represented as one immutable
        // source overlay and captured through the same fixed-view mechanism.
        // The projector then threads that exact view into a
        // `SessionResolverContext`; a profile with no content override uses the
        // base `HostViewRef` capture.
        let render = |fixed: &crate::resolver_store::BatchFixedView,
                      view: &dyn crate::session_view::SessionView| {
            self.render_public_api_items(
                std::slice::from_ref(&canonical_id),
                mode,
                profile,
                fixed,
                view,
            )
            .into_iter()
            .next()
            .unwrap_or(Ok(None))
        };

        let _ = profile;

        let view = crate::session_view::HostViewRef::new(self);
        let fixed = self.capture_batch_fixed_view(&view);
        render(&fixed, &view)
    }

    /// Generate the public declaration and its framework-owned structured
    /// contract from one coherent projector invocation.
    ///
    /// Editor consumers use the sidecar instead of reparsing generated
    /// declaration text. When structured metadata is unavailable, the
    /// declaration remains available with typed `Unsupported` contract
    /// availability.
    ///
    /// THIS entry is where the mandatory contract composes (demand-scoped):
    /// the declaration renders and the contract composes under ONE shared
    /// `(fixed, view)` capture. Response-only entries ([`Self::get_public_api`],
    /// [`Self::get_public_api_with_mode`], [`Self::get_public_api_batch`])
    /// render the declaration only and never run the component-meta walk.
    pub fn get_public_api_projection(
        &self,
        canonical_id: &str,
    ) -> Result<
        Option<crate::framework::api_projector::ComponentApiProjection>,
        crate::PublicApiProjectionError,
    > {
        let view = crate::session_view::HostViewRef::new(self);
        let fixed = self.capture_batch_fixed_view(&view);
        let response = self
            .render_public_api_items(
                std::slice::from_ref(&canonical_id),
                PublicApiMode::Public,
                None,
                &fixed,
                &view,
            )
            .into_iter()
            .next()
            .unwrap_or(Ok(None))?;
        let Some(response) = response else {
            return Ok(None);
        };
        // A rendered response proves the same classification chain the render
        // ran resolves; re-derive the adapter identity for the contract's
        // typed `Unsupported` arms.
        let canonical = self.resolve_alias_or_canonical(canonical_id);
        let Some(adapter_id) = self
            .scheduler
            .try_get_source(&canonical)
            .and_then(|snap| {
                snap.downcast_data::<crate::host_executor::HostSourceData>()
                    .map(|hd| hd.file_language.clone())
            })
            .and_then(|file_language| file_language.adapter_id().cloned())
        else {
            return Ok(None);
        };
        let contract = self.compose_component_contract(&canonical, &adapter_id, &view, &fixed);
        Ok(Some(
            crate::framework::api_projector::ComponentApiProjection { response, contract },
        ))
    }

    /// The Vue public-API extraction body — the EXEMPT legacy producer the
    /// `vue` component-API projector leg delegates to.
    ///
    /// Consumes deep pipeline internals (`cached_tsc_extract` /
    /// `extract_tsc_state` / `generate_tsc_from_state` / the request-local
    /// TypeInfo macro producer / `sync_transitive_macro_type_dependencies`) so
    /// it stays in this module.
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
    ) -> Result<Option<TscResponse>, crate::PublicApiProjectionError> {
        // Already alias-resolved by the caller; own it for the body's
        // existing `&canonical` / `.clone()` consumers without re-resolving.
        let canonical = resolved_canonical.to_string();
        let profile_hash = profile.map(compile_profile_hash);
        let has_content_override = false;

        if self.is_canonical_evicted(&canonical) {
            return Ok(None);
        }

        let Some((source, cached_extract, whole_hash)) = (|| {
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
            let cached = if has_content_override {
                None
            } else {
                self.derived_raw_cache().get(&canonical).and_then(|cc| {
                    cc.cached_tsc_extract.as_ref().and_then(|(hash, extract)| {
                        if *hash == efs.whole_hash {
                            Some(Arc::clone(extract))
                        } else {
                            None
                        }
                    })
                })
            };
            Some((efs.source, cached, efs.whole_hash))
        })() else {
            return Ok(None);
        };

        // Derive component name from canonical_id: last path segment, strip .vue extension.
        let component_name = canonical
            .rsplit('/')
            .next()
            .unwrap_or(&canonical)
            .trim_end_matches(".vue")
            .to_string();
        // Produce one terminal TSC bundle for this public-API request. Batch
        // callers share their captured cold seed; direct callers create one
        // coherent seed through the producer helper.
        let macro_output = if let Some(seed) = render_seed.as_ref() {
            let overlay =
                std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
            if seed.view.overlay_content_hash_for(&canonical).is_some() {
                let session_ctx = crate::resolver_core::SessionResolverContext::from_cold_seed(
                    self,
                    seed.view,
                    seed.cold_seed,
                    overlay,
                );
                self.produce_vue_macro_codegen_with_ctx(
                    &session_ctx,
                    &canonical,
                    crate::typeinfo::vue_macro_codegen::VueMacroCodegenDemand::Tsc,
                )
            } else {
                let host_ctx = crate::resolver_core::HostResolverContext::from_cold_seed(
                    self,
                    seed.cold_seed,
                    overlay,
                );
                self.produce_vue_macro_codegen_with_ctx(
                    &host_ctx,
                    &canonical,
                    crate::typeinfo::vue_macro_codegen::VueMacroCodegenDemand::Tsc,
                )
            }
        } else {
            self.produce_vue_macro_codegen(
                &canonical,
                crate::typeinfo::vue_macro_codegen::VueMacroCodegenDemand::Tsc,
            )
        };
        if !vue_macro_output_matches_revision(&macro_output, whole_hash) {
            return Ok(None);
        }
        // The syntax extract/source and semantic bundle must describe the same
        // indexed revision. Producing the semantic closure may overlap an edit;
        // fail closed instead of joining compatible-looking macro ordinals from
        // different source revisions.
        if self
            .effective_file_state(&canonical, profile_hash)
            .is_none_or(|current| current.whole_hash != whole_hash)
        {
            return Ok(None);
        }
        let transitive_macro_type_deps =
            macro_output.transitive_canonicals.iter().cloned().collect();
        // Unconditional: `replace_semantic_transitive(canonical, {})`
        // CLEARS the semantic axis when the set is empty.
        self.sync_transitive_macro_type_dependencies(&canonical, &transitive_macro_type_deps);
        let macro_tsc = macro_output.tsc;
        // The PARENT-FACING half of attribute fallthrough. `$attrs` already
        // answers what this component may READ; this is what a parent may
        // PASS, and it is the surface a consumer's `<Child title="…" />` is
        // checked against. Resolved by the single inheritance resolver
        // (`verter_session` owns it per the Fallthrough / Root Inheritance
        // CRITICAL rule) and handed to the compiler as data — the compiler
        // cannot see the resolver. Every uncertainty projects to "widen
        // nothing"; see `fallthrough_props`.
        //
        // The resolve is PINNED to the batch's captured fixed view when that
        // capture is still promotion-admissible, so this render takes ZERO
        // additional store-view reads — the O(1)-batch contract this path
        // documents. A capture that is no longer admissible falls back to the
        // resolver's own snapshot rather than validating a warm entry against
        // a stale view.
        //
        // The captured fingerprint travels WITH the view. The executor stamps
        // that captured value (never a fresh live read) and compares it against
        // the live fingerprint before promoting, so an import/root edit landing
        // after the one-shot admissibility precheck makes the result
        // return-only rather than warming a shared entry computed from an old
        // route view under new fact hashes.
        let pinned_view = render_seed
            .as_ref()
            .map(|seed| seed.fixed)
            .filter(|fixed| fixed.payload_promotion_admissible(self))
            .map(|fixed| {
                let (view, captured_fingerprint) = fixed.executor_fixed_view();
                (view, captured_fingerprint, fixed.is_current())
            });
        //
        // Root reachability is a TEMPLATE fact and `AnalysisScope::BUILD` — the
        // preset both shipping carrier producers run under — carries no
        // template flag. The resolve opens its own request-scoped demand for
        // exactly the files it walks; see
        // `resolve_fallthrough_surface_internal_with_overrides`.
        let fallthrough_resolution =
            self.resolve_fallthrough_surface_pinned(&canonical, pinned_view);
        let fallthrough_props = crate::host_resolve::fallthrough_props::project_fallthrough_props(
            fallthrough_resolution.as_ref(),
            &|child_canonical_id| self.owner_import_reference_for(&canonical, child_canonical_id),
        );
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
            if !has_content_override {
                // cached_tsc_extract lives on DerivedRawState (D48 split).
                let mut derived_ref = self.derived_raw_entry_or_default(canonical.clone());
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
                    mode: tsc_mode,
                },
                macro_tsc.as_deref().map_or(
                    verter_compiler::tsc::MacroTscInput::NotRequired,
                    verter_compiler::tsc::MacroTscInput::Authoritative,
                ),
                &fallthrough_props,
            )?;
            return Ok(Some(TscResponse::new(
                Arc::from(tsc_out.code),
                if tsc_out.source_map.is_empty() {
                    None
                } else {
                    Some(Arc::from(tsc_out.source_map))
                },
                tsc_out.dialect,
                tsc_out.ts_carrier_code.map(Arc::from),
            )));
        };

        let tsc_out = verter_compiler::tsc::generate_tsc_from_state(
            &extract,
            &component_name,
            tsc_mode,
            macro_tsc.as_deref().map_or(
                verter_compiler::tsc::MacroTscInput::NotRequired,
                verter_compiler::tsc::MacroTscInput::Authoritative,
            ),
            &fallthrough_props,
        )?;
        Ok(Some(TscResponse::new(
            Arc::from(tsc_out.code),
            if tsc_out.source_map.is_empty() {
                None
            } else {
                Some(Arc::from(tsc_out.source_map))
            },
            tsc_out.dialect,
            tsc_out.ts_carrier_code.map(Arc::from),
        )))
    }

    /// Store a compile's diagnostics ONLY while the live source is still the
    /// exact revision that compile read.
    ///
    /// `latest_diagnostics` is observable state a reader trusts as describing
    /// the CURRENT buffer: `get_diagnostics` is a pure cached read, and its LSP
    /// consumers stamp what they read with the document version they captured.
    /// An upsert clears the slot precisely because the old diagnostics no
    /// longer describe the file. So a compile that finishes AFTER a newer edit
    /// must not write into the state that edit just cleared — v2's parse errors
    /// landing over v3's cleared slot are then indistinguishable from v3's own,
    /// and a concurrent publisher that captured v3, read the slot and passed
    /// its own document-identity fence will publish them stamped `v3`.
    ///
    /// This mirrors the `Content`-mode publish decline: compare the live
    /// identity against the one captured with the compiled bytes and write only
    /// on a match. `whole_hash` is the right grain — ANY byte moving makes these
    /// diagnostics describe text no longer in the buffer, including a
    /// style-only edit that leaves `semantic_hash` alone.
    ///
    /// Refusing is safe and never strands a file without diagnostics: every
    /// path that moves the source also schedules a fresh compile for the
    /// revision that moved it (the document commit signals the coordinator,
    /// whose debounced tick compiles), so the newest revision always writes its
    /// own. A vanished live source declines the same way.
    ///
    /// Returns whether the write landed.
    pub(crate) fn store_latest_diagnostics_if_source_unmoved(
        &self,
        canonical_id: &str,
        profile_hash: u64,
        compiled_whole_hash: Hash16,
        diagnostics: DiagnosticsSnapshot,
    ) -> bool {
        // The identity check runs INSIDE the compile-cache entry guard, with
        // the write, and that is what makes it a fence rather than a hint.
        // Checked before acquiring the entry it would be TOCTOU: this compile
        // could observe its own revision, the upserting edit could then take
        // the entry and clear, and this write would land after that clear.
        //
        // Holding the entry across both closes it because the two orderings
        // inside `upsert` are fixed: the scheduler source commits
        // (`submit_batch_atomic` + `wait_batch`) BEFORE the compile-cache
        // clear takes this same entry. So under this guard, "the scheduler
        // still reports the compiled hash" implies the clear for any newer
        // revision has not run yet — and it necessarily runs after this write,
        // which erases it. The only other order, the clear having already run,
        // means the scheduler moved first and the check declines.
        let Some(mut cc) = self.compile_cache().get_mut(canonical_id) else {
            return false;
        };
        let live_whole_hash = self
            .scheduler
            .try_get_source(canonical_id)
            .and_then(|snap| {
                snap.downcast_data::<crate::host_executor::HostSourceData>()
                    .map(|live_hd| live_hd.parse.whole_hash)
            });
        if live_whole_hash != Some(compiled_whole_hash) {
            return false;
        }
        cc.latest_diagnostics.insert(profile_hash, diagnostics);
        cc.diagnostics_generation += 1;
        true
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn compile_entry(
        &self,
        snapshot: &CompileInput,
        profile: &CompileProfile,
    ) -> Result<CompileEntryOutcome, DiagnosticsSnapshot> {
        let mut diagnostics = snapshot.parse_diagnostics.clone();

        // Test-only observable: the per-file source re-clone the
        // RuntimeRender lane avoids for a simple (no external `src=`) file.
        // See `VerterHost::wrapper_source_clone_count`.
        #[cfg(test)]
        self.test_force
            .wrapper_source_clone_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let alloc = Allocator::new();

        // The macro bundle demand FOLLOWS the caller's target instead of
        // always asking for the heaviest one. A TSX-only (IDE) compile takes
        // the public binding names; only a target that renders the runtime
        // `props` option object pays for per-member broad-runtime
        // classification, which resolves every member's type through the
        // shared semantic engine. A Vue carrier always produces at least the
        // names bundle, because the shared payload resolution underneath it is
        // what yields this file's macro dependency diagnostics and its
        // transitive macro type dependencies.
        let macro_demand =
            crate::typeinfo::vue_macro_codegen::VueMacroCodegenDemand::for_compile_target(
                profile.target,
            )
            .unwrap_or(
                crate::typeinfo::vue_macro_codegen::VueMacroCodegenDemand::RuntimeBindingNames,
            );
        let is_vue = self
            .language_classifier()
            .classify(&snapshot.canonical_id)
            .is_vue();
        let macro_output =
            is_vue.then(|| self.produce_vue_macro_codegen(&snapshot.canonical_id, macro_demand));
        let macro_dependency_diagnostics = macro_output
            .as_ref()
            .map(|output| super::vue_macro_dependency_diagnostics::collect(self, snapshot, output))
            .unwrap_or_default();
        let transitive_macro_type_deps = macro_output
            .as_ref()
            .map(|output| output.transitive_canonicals.iter().cloned().collect())
            .unwrap_or_default();
        self.sync_transitive_macro_type_dependencies(
            &snapshot.canonical_id,
            &transitive_macro_type_deps,
        );
        if !macro_dependency_diagnostics.is_empty() {
            diagnostics =
                diagnostics.merge(DiagnosticsSnapshot::from_vec(macro_dependency_diagnostics));
            return Err(diagnostics);
        }

        let scope = self.config.effective_scope();

        // The host-resolved Vue cross-file inputs ride on the typed,
        // ephemeral `VueExecutionInputs` carrier — excluded from
        // `CompileRequest` identity, but no longer erased through an
        // `Arc<dyn Any>` downcast. A non-Vue carrier ignores it; Vue reads
        // it directly.
        let vue_facts = verter_compiler::compile::types::VueExecutionInputs {
            macro_runtime: macro_output.and_then(|output| output.runtime),
            prop_constness_overrides: None, // populated by the cross-file optimizer
            style_v_bind_vars: snapshot.style_v_bind_vars.clone(),
            style_v_bind_usage_complete: Some(snapshot.style_v_bind_usage_complete),
            template_binding_metadata: None,
            template_used_vars: None,
            runtime_template_hole: false,
            runtime_inline_template_chunk: false,
        };

        // The RUNTIME products are requested when the profile target
        // carries a runtime output bit. The target bits already participate
        // in `profile_hash`, so the publication identity carries the
        // requested-product set — no cache is re-keyed for this.
        let want_runtime = profile.target.needs_runtime_module();
        // IDE TSX is requested when the profile target carries the TSX bit.
        let want_ide = profile.target.needs_tsx();
        // Template facts are requested by the active analysis scope OR an
        // explicit TEMPLATE_DATA target bit. (The Vue runtime path always
        // requests `extract_template_data = scope.needs_template_analysis()`.)
        let want_template_data =
            scope.needs_template_analysis() || profile.target.needs_template_data();

        // The canonical, admission-checked request. This is the session's
        // per-file/virtual-product request-construction authority: an
        // unsupported option, a malformed Svelte namespace/fragments token,
        // or an SSR x Vapor / inline x SSR combination refuses HERE, before
        // any codegen input is built — see `build_compile_request`.
        let request = match build_compile_request(
            profile,
            &snapshot.canonical_id,
            is_vue,
            want_runtime,
            want_ide,
            want_template_data,
        ) {
            Ok(request) => request,
            Err(error) => {
                return Err(diagnostics.merge(request_construction_refused_diagnostics(
                    &snapshot.canonical_id,
                    snapshot.source.len() as u32,
                    &error,
                )));
            }
        };

        // The neutral runtime-compile options the carrier consults, read
        // back off the validated request — never re-derived from `profile`
        // directly. The framework-private resolved inputs ride on
        // `vue_facts`; the carrier reads only what it supports.
        let runtime_opts = derive_runtime_compile_options(
            &request,
            profile,
            snapshot.block_content_inputs.clone(),
            Some(vue_facts),
        );

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
                    arguments: Vec::new(),
                    span: verter_span::Span::new(0, snapshot.source.len() as u32),
                }])),
            );
        };
        let Some(compiler) = crate::parse::carrier_compiler_registry()
            .compiler_for_carrier_language(artifact.adapter_id(), artifact.language_id())
        else {
            return Err(
                diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
                    severity: HostSeverity::Error,
                    code: "HOST_NO_CARRIER_COMPILER".to_string(),
                    message: format!(
                        "no carrier compiler for adapter '{}' / language '{}'",
                        artifact.adapter_id().as_str(),
                        artifact.language_id().as_str()
                    ),
                    arguments: Vec::new(),
                    span: verter_span::Span::new(0, snapshot.source.len() as u32),
                }])),
            );
        };

        // The host OWNS the cached-parse validity decision: a cached
        // artifact is reused ONLY when the source was not modified by
        // external `src=` merging and the profile carries no
        // parse-affecting template options (custom delimiters / custom
        // elements). Otherwise the carrier re-parses the merged source.
        // Either way the carrier owns the typed downcast + native compile.
        let grammar_matches = profile
            .delimiters
            .as_ref()
            .is_none_or(|value| value.0 == "{{" && value.1 == "}}")
            && profile.custom_elements.as_ref().is_none_or(Vec::is_empty);
        if !grammar_matches {
            return Err(
                diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![HostDiagnostic {
                    severity: HostSeverity::Error,
                    code: "HOST_CARRIER_GRAMMAR_MISMATCH".to_string(),
                    message: "compile profile grammar differs from registered grammar".to_string(),
                    arguments: Vec::new(),
                    span: verter_span::Span::new(0, snapshot.source.len() as u32),
                }])),
            );
        }

        let outcome = match compiler.compile_bundle(
            snapshot.source.as_ref(),
            artifact,
            &runtime_opts,
            &alloc,
        ) {
            Ok(outcome) => outcome,
            Err(unsupported) => {
                let code = match unsupported {
                    CompileUnsupported::TargetMissingIde => "HOST_COMPILE_TARGET_MISSING_IDE",
                    CompileUnsupported::NoIdeProjection { .. } => "HOST_COMPILE_UNSUPPORTED",
                    CompileUnsupported::BlockContentRuntimeUnavailable { .. } => {
                        "HOST_BLOCK_CONTENT_RUNTIME_UNAVAILABLE"
                    }
                    CompileUnsupported::BlockContentIdeUnavailable { .. } => {
                        "HOST_BLOCK_CONTENT_IDE_UNAVAILABLE"
                    }
                    CompileUnsupported::RequestExecutionRefused(_) => {
                        "HOST_COMPILE_REQUEST_EXECUTION_REFUSED"
                    }
                };
                return Err(diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![
                    HostDiagnostic {
                        severity: HostSeverity::Error,
                        code: code.to_string(),
                        message: format!(
                            "carrier '{}' cannot produce a runtime bundle for '{}'",
                            artifact.adapter_id().as_str(),
                            snapshot.canonical_id
                        ),
                        arguments: Vec::new(),
                        span: verter_span::Span::new(0, snapshot.source.len() as u32),
                    },
                ])));
            }
        };

        // A carrier that fail-closed on the runtime surface THIS request asked
        // for returns a product-free refusal, and the transaction ends here: no
        // outputs are assembled, no IDE artifact is lifted, no template analysis
        // is built. Its non-fatal diagnostics still reach the host snapshot so
        // the reason is visible to the diagnostics route.
        let compiled = match outcome {
            CarrierCompileOutcome::Produced(bundle) => bundle,
            CarrierCompileOutcome::RuntimeSurfaceRefused(refusal) => {
                let mut refusal_diags = diagnostics;
                let mut lifted: Vec<HostDiagnostic> = refusal
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
                        arguments: Vec::new(),
                        span: d.span,
                    })
                    .collect();
                // The refusal's own reason, surfaced as the NON-FATAL diagnostic
                // it has always been: it is not a compile error, it is the
                // terminal answer for this request's runtime product.
                lifted.push(HostDiagnostic {
                    severity: HostSeverity::Warning,
                    code: refusal.diagnostic_code.clone(),
                    message: refusal.message.clone(),
                    arguments: Vec::new(),
                    span: refusal.span,
                });
                refusal_diags = refusal_diags.merge(DiagnosticsSnapshot::from_vec(lifted));
                return Ok(CompileEntryOutcome::RuntimeSurfaceRefused(
                    CompileEntryRefusal {
                        diagnostic_code: Arc::from(refusal.diagnostic_code.as_str()),
                        message: Arc::from(refusal.message.as_str()),
                        diagnostics: refusal_diags,
                    },
                ));
            }
        };

        // Lift the bundle's framework-neutral diagnostics into the host
        // `DiagnosticsSnapshot` (a Svelte projector diagnostic reaches the
        // snapshot through THIS path). DEDUPLICATED against `diagnostics`
        // (the carrier's own parse-time channel, already in `compile_diags`):
        // Vue's `compile_bundle` reuses the already-parsed artifact and its
        // compile result clones that SAME `ParsedSfc`'s diagnostics wholesale
        // (`clone_diagnostics`), so a parse-time diagnostic
        // (`MissingSfcEntryBlock`) is otherwise double-counted — see
        // `DiagnosticsSnapshot::merge_deduplicated`'s doc.
        let mut compile_diags = diagnostics.clone();
        if !compiled.diagnostics.is_empty() {
            compile_diags = compile_diags.merge_deduplicated(DiagnosticsSnapshot::from_vec(
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
                        arguments: Vec::new(),
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
        // `get_virtual_file(Main)` then reports missing until that carrier
        // emits a runtime surface.
        // PUBLICATION is per-product: a virtual node enters `outputs` only when
        // its own bit is in the request's target. The compile may legitimately
        // produce more than that — Vue's template-data extraction runs script
        // codegen as a PREREQUISITE, and a carrier's scoped CSS comes out of the
        // same runtime compile as its module — but a prerequisite is not a
        // product, so it never enters the published set. This gates at the point
        // of insertion rather than filtering an assembled map, so an unrequested
        // product is never published in the first place.
        let publish_runtime_module = profile.target.publishes_runtime_module();
        let publish_script = profile.target.contains(CompileTarget::SCRIPT);
        let publish_template = profile.target.contains(CompileTarget::TEMPLATE);
        let publish_style = profile.target.needs_style();

        if publish_runtime_module && compiled.has_runtime_surface() {
            let (main_code, main_source_map, main_lang) = match &compiled.main.body_code {
                // A carrier that emits its own self-contained ESM body uses it
                // verbatim (e.g. Svelte's official-shaped runtime output),
                // paired with the map that carrier produced for it.
                Some(body) => (
                    body.clone(),
                    (!compiled.main.source_map.is_empty())
                        .then(|| Arc::from(compiled.main.source_map.clone())),
                    compiled.main.lang.clone().unwrap_or_else(|| {
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
                    }),
                ),
                // Vue: the host assembles the `_sfc_main` module from the
                // neutral block fields (its virtual-file concern) — and the map
                // it composed while assembling them. The code and the map are
                // one result of one assembly, so the map here always describes
                // the exact code beside it. `assembled.lang` is the SAME
                // dialect the assembler derived once and validated every
                // fragment/the final artifact under — reused here instead of
                // independently re-deriving it a second time.
                None => {
                    let assembled = assemble_vue_main_module(
                        &snapshot.canonical_id,
                        &compiled,
                        &snapshot.meta,
                        profile,
                    )
                    .map_err(|failure| {
                        assembled_map_failure_diagnostics(failure, snapshot.source.len() as u32)
                    })?;
                    (
                        assembled.code,
                        assembled.source_map.map(Arc::from),
                        assembled.lang,
                    )
                }
            };
            outputs.insert(
                VirtualNodeKind::Main,
                CachedVirtualFile {
                    code: Arc::from(main_code),
                    source_map: main_source_map,
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

        if let Some(script) = compiled.script.filter(|_| publish_script) {
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

        if let Some(template) = compiled.template.filter(|_| publish_template) {
            let (code, source_map) =
                compose_template_virtual_file(template, profile.runtime_module_name.as_deref())
                    .map_err(|refusal| {
                        template_compose_refusal_diagnostics(refusal, snapshot.source.len() as u32)
                    })?;
            outputs.insert(
                VirtualNodeKind::Template,
                CachedVirtualFile {
                    code: Arc::from(code),
                    source_map: source_map.map(Arc::from),
                    lang: Some("tsx".to_string()),
                    meta: VirtualMeta::default(),
                },
            );
        }

        for (i, style) in compiled
            .styles
            .into_iter()
            .enumerate()
            .filter(|_| publish_style)
        {
            // The compiler-produced CSS and map already reflect the single
            // host-selected block artifact. There is no ordinal override
            // layer at this boundary.
            let style_source_map: Option<Arc<str>> = style.source_map.map(Arc::from);
            outputs.insert(
                VirtualNodeKind::Style { index: i },
                CachedVirtualFile {
                    code: Arc::from(style.code),
                    source_map: style_source_map,
                    lang: Some(style.lang.unwrap_or_else(|| "css".to_string())),
                    meta: VirtualMeta {
                        style_index: Some(i),
                        ..VirtualMeta::default()
                    },
                },
            );
        }

        // Custom blocks have no target bit of their own. They ride with the
        // runtime module: the host's `Main` assembly emits their virtual imports
        // (`crate::compile::assemble_vue_main_module`), so a `Custom` node with
        // no `Main` would have no importer.
        for (i, block) in compiled
            .custom_blocks
            .into_iter()
            .enumerate()
            .filter(|_| publish_runtime_module)
        {
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
        let mut template_class_admission =
            crate::project_semantic_dispatch::template_class_facts::TemplateClassCacheAdmission::not_applicable();
        let template_analysis = compiled.template_data.as_ref().map(|raw| {
            // Build script import pairs for component â†’ source resolution
            let all_imports =
                template_converter_inputs(&snapshot.script_imports, &snapshot.script_bindings);
            let facts = self.build_template_class_semantic_facts(
                &snapshot.canonical_id,
                snapshot.whole_hash,
                Arc::clone(&snapshot.source),
                crate::project_semantic_dispatch::template_class_facts::TemplateClassScriptInputs {
                    macros: &snapshot.script_macros,
                    bindings: &snapshot.script_bindings,
                },
                raw,
                // The compile lane's bytes attestation: an override layer is a
                // fenced input, plain snapshot bytes are store-published. The
                // seed-currentness half is composed inside the wrapper.
                crate::project_semantic_dispatch::template_class_facts::TemplateClassPublicationScope::BasePublishable,
            );
            let class_domains =
                crate::template_convert::TemplateClassDomainIndex::from_semantic_facts(
                    &facts,
                    &snapshot.canonical_id,
                    snapshot.whole_hash,
                )
                .unwrap_or_else(crate::template_convert::TemplateClassDomainIndex::empty);
            template_class_admission =
                crate::project_semantic_dispatch::template_class_facts::TemplateClassCacheAdmission::from_facts(&facts);
            let unused_ctx = crate::template_convert::UnusedDeclarationContext::from_analysis(
                &snapshot.script_macros,
                snapshot.script_macro_usage.as_ref(),
                &snapshot.script_vue_api_calls,
                &snapshot.script_bindings,
                // The compile input style v-bind vars are the SOUND OXC-derived
                // roots the analysis snapshot records on each analyzed v-bind.
                &snapshot.style_v_bind_vars,
            );
            crate::template_convert::convert_raw_to_analysis(
                raw,
                &all_imports,
                &class_domains,
                Some(&unused_ctx),
            )
        });

        Ok(CompileEntryOutcome::Produced(CompileEntryProducts {
            outputs,
            diagnostics: compile_diags,
            tsx: cached_tsx,
            template_analysis,
            template_class_admission,
        }))
    }

    /// Render-only sibling of [`Self::compile_entry`]: produces the SAME
    /// `Main` bytes through the SAME shared substrate (`compile_bundle`) and
    /// the SAME host-side [`assemble_vue_main_module`], WITHOUT the per-file
    /// session-wrapper overhead. Returns the assembled `Main` code, its
    /// optional source map, and warning-severity diagnostics of a successful
    /// render.
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
    /// - (f) one request-local runtime macro bundle is produced from TypeInfo;
    ///   the render lane does not retain it or mutate dependency state.
    #[cfg(not(target_arch = "wasm32"))]
    fn compile_entry_runtime_render(
        &self,
        snapshot: &CompileInput,
        profile: &CompileProfile,
    ) -> Result<RenderOnlyMain, HostError> {
        let diagnostics = snapshot.parse_diagnostics.clone();

        // (a) DROP the source re-clone for the common case. Only the
        // external-`src=` merge (rare, and inherently allocating) builds an
        // owned String; otherwise the substrate borrows the snapshot bytes.

        // The compiler's own parse scratch. A local `Allocator` per render
        // call passed straight into `compile_bundle` is NOT carrier-lifecycle
        // state; it is transient parse scratch, dropped at the end of this
        // call.
        let alloc = Allocator::new();

        let macro_output = self.produce_vue_macro_codegen(
            &snapshot.canonical_id,
            crate::typeinfo::vue_macro_codegen::VueMacroCodegenDemand::Runtime,
        );

        let scope = self.config.effective_scope();

        let vue_facts = verter_compiler::compile::types::VueExecutionInputs {
            macro_runtime: macro_output.runtime,
            prop_constness_overrides: None,
            style_v_bind_vars: snapshot.style_v_bind_vars.clone(),
            style_v_bind_usage_complete: Some(snapshot.style_v_bind_usage_complete),
            template_binding_metadata: None,
            template_used_vars: None,
            runtime_template_hole: false,
            runtime_inline_template_chunk: false,
        };

        // The render lane's whole subject is the runtime `Main` module, so
        // it always asks for the runtime products regardless of the
        // caller's target bits — mirroring the lane's own contract
        // (`CompileManyTarget::RuntimeRender`), not the profile's.
        let want_runtime = true;
        let want_ide = profile.target.needs_tsx();
        let want_template_data =
            scope.needs_template_analysis() || profile.target.needs_template_data();

        // The canonical, admission-checked request — same construction
        // authority as `compile_entry` (this lane is Vue-only, matching its
        // own module contract). A refusal here is FATAL, matching every
        // other construction-time site this lane already treats as fatal.
        let request = match build_compile_request(
            profile,
            &snapshot.canonical_id,
            true,
            want_runtime,
            want_ide,
            want_template_data,
        ) {
            Ok(request) => request,
            Err(error) => {
                return Err(HostError::CompileError(CompileFailure {
                    diagnostics: diagnostics.merge(request_construction_refused_diagnostics(
                        &snapshot.canonical_id,
                        snapshot.source.len() as u32,
                        &error,
                    )),
                    requested_mode: profile.requested_mode,
                    actual_mode: profile.requested_mode,
                    downgrade_reason: None,
                }));
            }
        };

        // The compiler-visible runtime options, read back off the
        // validated request — byte-identical construction to
        // `compile_entry`'s.
        let runtime_opts = derive_runtime_compile_options(
            &request,
            profile,
            snapshot.block_content_inputs.clone(),
            Some(vue_facts),
        );

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
                        arguments: Vec::new(),
                        span: verter_span::Span::new(0, snapshot.source.len() as u32),
                    },
                ])),
                requested_mode: profile.requested_mode,
                actual_mode: profile.requested_mode,
                downgrade_reason: None,
            }));
        };
        let Some(compiler) = crate::parse::carrier_compiler_registry()
            .compiler_for_carrier_language(artifact.adapter_id(), artifact.language_id())
        else {
            return Err(HostError::CompileError(CompileFailure {
                diagnostics: diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![
                    HostDiagnostic {
                        severity: HostSeverity::Error,
                        code: "HOST_NO_CARRIER_COMPILER".to_string(),
                        message: format!(
                            "no carrier compiler for adapter '{}' / language '{}'",
                            artifact.adapter_id().as_str(),
                            artifact.language_id().as_str()
                        ),
                        arguments: Vec::new(),
                        span: verter_span::Span::new(0, snapshot.source.len() as u32),
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
        let grammar_matches = profile
            .delimiters
            .as_ref()
            .is_none_or(|value| value.0 == "{{" && value.1 == "}}")
            && profile.custom_elements.as_ref().is_none_or(Vec::is_empty);
        if !grammar_matches {
            return Err(HostError::CompileError(CompileFailure {
                diagnostics: diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![
                    HostDiagnostic {
                        severity: HostSeverity::Error,
                        code: "HOST_CARRIER_GRAMMAR_MISMATCH".to_string(),
                        message: "compile profile grammar differs from registered grammar"
                            .to_string(),
                        arguments: Vec::new(),
                        span: verter_span::Span::new(0, snapshot.source.len() as u32),
                    },
                ])),
                requested_mode: profile.requested_mode,
                actual_mode: profile.requested_mode,
                downgrade_reason: None,
            }));
        }

        let compiled = match compiler.compile_bundle(
            snapshot.source.as_ref(),
            artifact,
            &runtime_opts,
            &alloc,
        ) {
            // The render lane's whole subject is the runtime `Main`, so a
            // refusal is simply the absence of the thing it was asked to
            // render — the same typed outcome the HostBacked route reports.
            Ok(CarrierCompileOutcome::RuntimeSurfaceRefused(refusal)) => {
                return Err(HostError::RuntimeSurfaceRefused {
                    canonical_id: snapshot.canonical_id.clone(),
                    diagnostic_code: refusal.diagnostic_code,
                    message: refusal.message,
                });
            }
            Ok(CarrierCompileOutcome::Produced(bundle)) => bundle,
            // Site 5 (`CompileUnsupported`) stays FATAL.
            Err(unsupported) => {
                let code = match unsupported {
                    CompileUnsupported::TargetMissingIde => "HOST_COMPILE_TARGET_MISSING_IDE",
                    CompileUnsupported::NoIdeProjection { .. } => "HOST_COMPILE_UNSUPPORTED",
                    CompileUnsupported::BlockContentRuntimeUnavailable { .. } => {
                        "HOST_BLOCK_CONTENT_RUNTIME_UNAVAILABLE"
                    }
                    CompileUnsupported::BlockContentIdeUnavailable { .. } => {
                        "HOST_BLOCK_CONTENT_IDE_UNAVAILABLE"
                    }
                    CompileUnsupported::RequestExecutionRefused(_) => {
                        "HOST_COMPILE_REQUEST_EXECUTION_REFUSED"
                    }
                };
                return Err(HostError::CompileError(CompileFailure {
                    diagnostics: diagnostics.merge(DiagnosticsSnapshot::from_vec(vec![
                        HostDiagnostic {
                            severity: HostSeverity::Error,
                            code: code.to_string(),
                            message: format!(
                                "carrier '{}' cannot produce a runtime bundle for '{}'",
                                artifact.adapter_id().as_str(),
                                snapshot.canonical_id
                            ),
                            arguments: Vec::new(),
                            span: verter_span::Span::new(0, snapshot.source.len() as u32),
                        },
                    ])),
                    requested_mode: profile.requested_mode,
                    actual_mode: profile.requested_mode,
                    downgrade_reason: None,
                }));
            }
        };

        // Lift the bundle's framework-neutral diagnostics into the host
        // snapshot. Semantic producer failures fail closed on every compile
        // lane; there is no render-only degradation policy. DEDUPLICATED
        // against `diagnostics` for the same reason as `compile_entry` — see
        // `DiagnosticsSnapshot::merge_deduplicated`'s doc.
        let mut compile_diags = diagnostics.clone();
        let mut compiled_diags: Vec<HostDiagnostic> = Vec::new();
        for d in &compiled.diagnostics {
            let severity = match d.severity {
                RuntimeDiagnosticSeverity::Error => HostSeverity::Error,
                RuntimeDiagnosticSeverity::Warning => HostSeverity::Warning,
                RuntimeDiagnosticSeverity::Info => HostSeverity::Info,
            };
            compiled_diags.push(HostDiagnostic {
                severity,
                code: d.code.clone(),
                message: d.message.clone(),
                arguments: Vec::new(),
                span: d.span,
            });
        }
        if !compiled_diags.is_empty() {
            compile_diags =
                compile_diags.merge_deduplicated(DiagnosticsSnapshot::from_vec(compiled_diags));
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
        let (main_code, main_source_map, main_lang) = match &compiled.main.body_code {
            Some(body) => (
                body.clone(),
                (!compiled.main.source_map.is_empty())
                    .then(|| Arc::from(compiled.main.source_map.clone())),
                // The `Main` language, derived IDENTICALLY to the HostBacked
                // `Main`-node path so the bundler consumer routes
                // sub-requests the same way.
                compiled.main.lang.clone().unwrap_or_else(|| {
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
                }),
            ),
            // Vue: code and map are one result of the host's own assembly.
            // `assembled.lang` is the SAME dialect the assembler derived
            // once and validated every fragment/the final artifact under.
            None => {
                let assembled = assemble_vue_main_module(
                    &snapshot.canonical_id,
                    &compiled,
                    &snapshot.meta,
                    profile,
                )
                .map_err(|failure| {
                    HostError::CompileError(CompileFailure {
                        diagnostics: assembled_map_failure_diagnostics(
                            failure,
                            snapshot.source.len() as u32,
                        ),
                        requested_mode: profile.requested_mode,
                        actual_mode: profile.requested_mode,
                        downgrade_reason: None,
                    })
                })?;
                (
                    assembled.code,
                    assembled.source_map.map(Arc::from),
                    assembled.lang,
                )
            }
        };

        Ok(RenderOnlyMain {
            code: Arc::from(main_code),
            source_map: main_source_map,
            lang: Some(main_lang),
            diagnostics: compile_diags
                .diagnostics
                .into_iter()
                .filter(|diagnostic| diagnostic.severity != HostSeverity::Error)
                .collect(),
        })
    }
}
