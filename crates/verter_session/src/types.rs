#[cfg(feature = "session_metrics")]
use std::collections::BTreeMap;
#[cfg(feature = "session_metrics")]
use std::collections::HashMap;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use thiserror::Error;

/// 128-bit hash (xxh3) stored as a byte array, used for content and semantic hashing.
pub type Hash16 = [u8; 16];

/// Discriminates between Vue Single File Components and other file types
/// (e.g. `.ts` files tracked for cross-file type resolution).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileKind {
    /// A `.vue` Single File Component — parsed into script/template/style blocks.
    VueSfc,
    /// A non-SFC file (`.ts`, `.js`, etc.) — tracked for dependency and export signatures.
    NonSfc,
}

impl FileKind {
    /// Infer file kind from a file path's extension.
    /// Files ending in `.vue` are `VueSfc`; everything else is `NonSfc`.
    pub fn from_path(path: &str) -> Self {
        if path.ends_with(".vue") {
            Self::VueSfc
        } else {
            Self::NonSfc
        }
    }
}

/// Hot Module Replacement strategy injected into the assembled main module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HmrStrategy {
    /// No HMR code is emitted.
    None,
    /// Vite-style HMR (`import.meta.hot`).
    Vite,
    /// Webpack-style HMR (`module.hot`).
    Webpack,
}

/// Controls behavior when compilation fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompileErrorPolicy {
    /// Always return an error on compile failure.
    StrictError,
    /// In dev mode, serve the last known good output (with `stale: true`)
    /// instead of returning an error. Falls back to error if no good output exists.
    DevServeLastKnownGood,
}

/// Controls how much static analysis is performed during upsert().
///
/// **Deprecated**: Prefer [`AnalysisScope`](verter_semantic::analysis::AnalysisScope) bitflags
/// for fine-grained control. This enum is retained for FFI backwards compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisLevel {
    /// Full analysis: OXC script parsing + lightningcss style analysis.
    /// Use for LSP (auto-complete, diagnostics, hover, etc.).
    Full,
    /// Essential analysis: OXC script parsing only (imports, macros, bindings).
    /// Skips style analysis. Use for bundler mode where smart invalidation
    /// needs import/macro data but style analysis is wasted work.
    Essential,
    /// No extra analysis. Only the SFC tokenization and hashing needed for
    /// compilation are performed. Smart invalidation falls back to Tier 1
    /// (full invalidation on any dependency change).
    None,
}

impl AnalysisLevel {
    /// Convert this legacy level to the equivalent [`AnalysisScope`] flags.
    pub fn to_scope(self) -> verter_semantic::analysis::AnalysisScope {
        match self {
            Self::Full => verter_semantic::analysis::AnalysisScope::LSP,
            Self::Essential => verter_semantic::analysis::AnalysisScope::ESSENTIAL,
            Self::None => verter_semantic::analysis::AnalysisScope::NONE,
        }
    }
}

/// Configuration for a [`VerterHost`](crate::VerterHost) instance.
#[derive(Debug, Clone)]
pub struct HostConfig {
    /// Whether the host is running in development mode.
    /// Enables features like `DevServeLastKnownGood` and `__file` injection.
    pub dev_mode: bool,
    /// How to handle compile errors (strict vs. last-known-good fallback).
    pub compile_error_policy: CompileErrorPolicy,
    /// URI scheme prefix for LSP virtual file IDs (e.g. `"verter-virtual"`).
    pub lsp_scheme: String,
    /// Maximum number of compile profiles cached per file. Excess profiles
    /// are evicted LRU by `last_access_tick`.
    pub max_profiles_per_file: usize,
    /// Extensions to try when matching extensionless import specifiers
    /// (e.g. `import './types'`) against canonical IDs with extensions
    /// (e.g. `/src/types.ts`). Order determines priority.
    pub resolve_extensions: Vec<String>,
    /// Controls how much static analysis runs during upsert().
    ///
    /// - `Full`: script + style analysis (for LSP)
    /// - `Essential`: script analysis only (for bundler — smart invalidation)
    /// - `None`: no extra analysis (only SFC parse for code_gen)
    ///
    /// When not `Full`, `get_analysis()` computes missing data on demand.
    ///
    /// **Deprecated**: Prefer `analysis_scope` for fine-grained control.
    /// If `analysis_scope` is `Some`, it takes precedence over this field.
    pub analysis_level: AnalysisLevel,

    /// Bitwise flags controlling which analysis passes run during upsert.
    ///
    /// When `Some`, takes precedence over `analysis_level`. When `None`,
    /// falls back to `analysis_level.to_scope()`.
    ///
    /// Use preset constants for common configurations:
    /// - [`AnalysisScope::BUILD`](verter_semantic::analysis::AnalysisScope::BUILD) — minimal for compilation
    /// - [`AnalysisScope::LSP`](verter_semantic::analysis::AnalysisScope::LSP) — full for IDE features
    /// - [`AnalysisScope::LINTER`](verter_semantic::analysis::AnalysisScope::LINTER) — for lint rules
    ///
    /// **Migration**: Prefer [`QueryProfile`](verter_semantic::profile::QueryProfile) via
    /// [`from_query_profile()`](Self::from_query_profile) which sets both this and the
    /// session-level query profile automatically.
    pub analysis_scope: Option<verter_semantic::analysis::AnalysisScope>,
    /// Enable shared Rust-side generic root propagation for fallthrough resolution.
    ///
    /// When enabled, the host may specialize child root targets from
    /// statically resolvable call-site prop types.
    pub generic_root_propagation: bool,
    /// Enable the Rust-first native audit surface for component-meta requests.
    /// When true, timing/memory/store snapshots are captured and emitted as
    /// structured `RequestAuditRecord` data. Default: false (zero overhead).
    pub audit_enabled: bool,
    /// Enable semantic-footprint capture. Requires `audit_enabled = true`.
    /// When true, each audited request attaches a
    /// `RequestFootprintAudit` to its record, populated from a
    /// per-request accumulator. Default: false.
    pub footprint_capture: bool,
    /// Enable per-file timing capture
    /// (`FileAudit::read_ms` / `parse_ms` / `lower_ms`) AND the
    /// host-owned peak-RSS sampler thread. Requires
    /// `audit_enabled = true`.
    ///
    /// When `false`, per-file timings stay `None` even on entries
    /// the request triggered, the workspace / executor
    /// instrumentation stays on the zero-cost path, and the
    /// peak-RSS sampler does not spawn — `process_rss_peak_bytes`
    /// stays at `0`.
    ///
    /// When `true` (native only), the host-owned peak-RSS sampler
    /// thread spawns on the first audit-enabled request, ticks
    /// every 50 ms over the active-request registry, and writes
    /// `fetch_max(current_process_rss())` into each in-flight
    /// request's per-request peak slot. The slot value lands in
    /// `RequestAuditRecord::memory.process_rss_peak_bytes` at
    /// finalize time.
    ///
    /// On WASM the sampler thread does not exist
    /// (`#[cfg(not(target_arch = "wasm32"))]`); the peak slot stays
    /// at `0` regardless of flag state. Default: `false`.
    pub audit_timing_capture: bool,
    /// Upper bound on derivation-subgraph edges captured per request.
    /// The miner truncates at this count and sets
    /// `graph_completeness.has_orphan_edges = true`. Default: 10_000.
    pub max_derivation_edges: usize,
    /// Depth budget for path-projection / dispatch traversals
    /// (supplement §5.D.0 r17 + §0.6.5 stack-depth
    /// discipline). Tests construct constrained hosts to exercise
    /// budget-exceeded sentinel paths (`HostConfig { depth_budget: 2,
    /// ..Default::default() }`). When `0`, the existing `MAX_DEPTH`
    /// fall-back kicks in; non-zero values cap path traversal depth
    /// at the constructor-time value.
    ///
    /// Constructor-time per §0.6.5: callers may NOT mutate this on
    /// an existing host. Default: `MAX_DEPTH` (the
    /// `component_meta_materialize` cap).
    pub depth_budget: usize,
    /// Projection-operation budget for path-projection / dispatch
    /// traversals (request-scoped fuse state
    /// promoted to host-owned state per §0.6.5 stack-depth
    /// discipline).
    ///
    /// The legacy engine carried this as `FuseBudgets::projection_op_count`,
    /// a per-engine-construction-scoped fuse rail (§1.4) that
    /// terminates utility-shape recursion (`Partial<T>` / `Pick<T,K>` /
    /// etc.) before recursion exhausts the call stack.
    /// Promotes the BUDGET (not the per-request COUNTER — that lives
    /// in the request-scoped `RequestBudget` accessed via TLS) to a
    /// constructor-time `HostConfig` field so dispatch consumers
    /// observe the same cap.
    ///
    /// Constructor-time per §0.6.5: callers may NOT mutate this on
    /// an existing host. Default: `2000` (the legacy
    /// `FuseBudgets::projection_op_count` default).
    ///
    /// When `0`, the legacy `FuseBudgets::default()` value is used as
    /// a fall-back so existing tests that construct a `HostConfig`
    /// without setting this field continue to observe the documented
    /// 2000-op cap.
    pub projection_op_budget: usize,
    /// Eviction-policy tunables for the project-global cache cluster.
    ///
    /// The default policy is **D33 live-content reachability only** —
    /// `memory_pressure_threshold == usize::MAX`, so no caller ever
    /// passes `memory_pressure: true` to
    /// [`crate::project_type_store::ProjectTypeStore::evict_unreachable_artifacts`]
    /// in default builds. The LRU floor path is preserved as an unused
    /// capability for production callers that want to opt in
    /// (out-of-plan-scope per D119).
    pub eviction_policy: EvictionPolicyConfig,
    /// Per-method timeout budgets for audited LSP handlers. Each
    /// `*_with_audit` LSP handler wraps its work in a budget
    /// timeout; on expiry the handler finalises the audit
    /// registration with `error: Some("cancelled")` so the leak guard
    /// remains discriminating against superseded requests.
    ///
    /// Only consulted when `audit_enabled = true`. Default values
    /// match LSP-server hover/completion responsiveness expectations
    /// (see [`LspMethodTimeoutsConfig::default`]).
    pub lsp_method_timeouts: LspMethodTimeoutsConfig,
    /// Override hooks for resolver budgets. Used by tests to drive
    /// the synthesis path into deliberate budget breaches without
    /// mutating the global default budget.
    pub recursion_budget_overrides: RecursionBudgetOverrides,
    /// Capacity of the host-owned typeinfo scratch cache used by
    /// [`crate::VerterHost::evaluate_type_expression_with_audit`].
    ///
    /// `None` selects the default capacity (64 per
    /// `crate::typeinfo::scratch_cache::DEFAULT_CAPACITY`).
    /// `Some(0)` disables the cache (every cacheable request
    /// becomes a one-shot synthesis). Other values cap the LRU at
    /// the chosen size — used by the `@verter/typeinfo` LRU
    /// eviction tests.
    pub typeinfo_scratch_cache_capacity: Option<usize>,
}

/// Test / advanced-tuning hooks for resolver budgets. Each field is
/// `Option<u32>` and overrides the default budget when `Some`. The
/// default `RecursionBudgetOverrides::default()` leaves every budget
/// at its production default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecursionBudgetOverrides {
    /// Override for the per-request synthesis recursion budget.
    /// Used by the slot-binding regression
    /// `slot_bindings_skip_cache_on_budget_exceeded` to drive
    /// `ComponentMetaResultDb` publication into the suppression
    /// path. Production code leaves this `None` so the resolver
    /// runs with its normal projection budget.
    pub synthesis_steps: Option<u32>,
    /// Override for the shallow walker's pathological-input cap
    /// (`PATHOLOGICAL_CAP` in `project_semantic_dispatch::walk`).
    /// When `Some(n)`, the walker emits a
    /// `walker_pathological_input_cap` warn event and sets
    /// `cache_suppress = true` once `visited.len() >= n`.
    ///
    /// Production code leaves this `None` so the walker runs at the
    /// 10_000-node default. Tests use a small override (e.g. `50`)
    /// to drive the cap-fire path on a hermetic fixture without
    /// requiring a 10_000-node corpus.
    pub walker_pathological_cap: Option<usize>,
}

/// Eviction policy tunables for the project-global cache cluster.
///
/// Live-content reachability is the default and sole eviction
/// mechanism: any cached `IndexedReady` entry whose
/// `(canonical_id, content_hash)` pair is not in the live publish set
/// is dropped from the host on every reachability sweep.
///
/// The LRU floor is a *secondary* mechanism that runs only under
/// explicit memory pressure. The decision to enter that path is left
/// to the caller (the resolver / scheduler can cite a real memory
/// pressure signal); the host does not auto-detect.
///
/// `memory_pressure_threshold` defaults to `usize::MAX` so no
/// production code path ever pays the LRU compute cost in default
/// builds. The threshold exists so production deployments can opt in
/// without a code change.
///
/// `per_canonical_content_hash_retention` (R20-adjacent) bounds
/// the number of distinct `content_hash` variants the host keeps
/// for a single canonical id. Older variants beyond this count are
/// dropped on the next reachability sweep regardless of whether
/// they are still in the live publish set — this is the concrete
/// memory-bound (R22) for "what happens after a long-lived session
/// edits the same file 100 times".
///
/// `promote_threshold` is the per-canonical hit count required
/// before an entry is "hot" for LRU-floor purposes.
/// Entries below the threshold age out first under memory pressure
/// even when their `last_access` tick is newer than a hot entry's;
/// hot entries survive the floor unless every entry has been
/// promoted.
#[derive(Debug, Clone)]
pub struct EvictionPolicyConfig {
    /// Memory-pressure threshold above which a caller may pass
    /// `memory_pressure: true` to
    /// [`crate::project_type_store::ProjectTypeStore::evict_unreachable_artifacts`].
    /// Default `usize::MAX` — no caller ever enters the LRU floor
    /// path. The host does not auto-detect memory pressure; callers
    /// are responsible for citing the trigger.
    ///
    /// Per D119: D33 live-content reachability is the sole eviction
    /// mechanism in default builds; LRU is preserved as an unused
    /// capability. Production deployments may override the threshold
    /// to opt in.
    pub memory_pressure_threshold: usize,
    /// Minimum number of cached entries the LRU floor preserves when
    /// it runs. Only consulted on the `memory_pressure: true` branch
    /// of [`crate::project_type_store::ProjectTypeStore::evict_unreachable_artifacts`].
    /// Default `1024`.
    pub min_floor: usize,
    /// Per-canonical `content_hash` variant retention. The
    /// reachability sweep keeps at most this many variants per
    /// canonical regardless of liveness; older variants are
    /// dropped. Default `3` — covers the {current, previous, baseline}
    /// window typical of an interactive editor.
    ///
    /// Setting `0` means "keep only the most recently touched
    /// variant"; setting `usize::MAX` disables the per-canonical
    /// cap (only the global LRU floor + reachability sweep are
    /// active). Binds R22 (eviction is memory-bound, not
    /// correctness-bound) — concurrent variants are still
    /// correctness-preserving via fact-validation, the cap only
    /// bounds the steady-state working set.
    pub per_canonical_content_hash_retention: usize,
    /// Promotion threshold: number of warm-hit observations
    /// required before an entry is considered "hot" for LRU-floor
    /// eviction. Default `2`.
    ///
    /// Hot entries survive the LRU floor unless every entry has
    /// been promoted; cold entries (below the threshold) age out
    /// first regardless of `last_access` recency. Mirrors the
    /// "two-strikes-and-you're-warm" pattern: a cache entry must be
    /// hit at least `promote_threshold` times before it is treated
    /// as a long-lived candidate.
    ///
    /// Setting `0` disables promotion (every entry counts as
    /// hot — LRU floor falls back to pure recency); setting
    /// `usize::MAX` disables promotion in the other direction
    /// (no entry is ever hot — LRU floor falls back to pure
    /// recency too, because the predicate is never satisfied).
    pub promote_threshold: u32,
}

/// Per-method timeout budgets for audited LSP handlers.
///
/// Each `*_with_audit` LSP handler wraps its work in a budget
/// timeout. On expiry (or on receipt of an LSP `$/cancelRequest`
/// translated to a deadline elapse), the handler finalises the
/// audit registration with the cancellation marker
/// `LspRequestPayload { error: Some("cancelled".to_string()), .. }`.
/// This makes superseded LSP requests observable in the audit
/// records store rather than leaking entries in the active-request
/// registry.
///
/// All values are durations. `Duration::ZERO` disables the timeout
/// for that method (the handler runs to completion); production
/// builds should retain the defaults so the leak guard is exercised
/// continuously.
#[derive(Debug, Clone)]
pub struct LspMethodTimeoutsConfig {
    /// `textDocument/hover` — fast lookup; typing-driven supersede
    /// dominates the workload, so the budget is tight.
    pub hover: std::time::Duration,
    /// `textDocument/definition` and `textDocument/typeDefinition`.
    pub goto_definition: std::time::Duration,
    /// `textDocument/completion` — wider budget because completion
    /// frequently round-trips through the type provider.
    pub completion: std::time::Duration,
    /// `textDocument/references` — workspace-wide search, larger
    /// budget than the position-bound methods.
    pub references: std::time::Duration,
    /// `textDocument/diagnostics` — push-diagnostics path; bounded
    /// by the debounce upstream but capped here to keep the leak
    /// guard discriminating.
    pub diagnostics: std::time::Duration,
    /// `textDocument/documentSymbol`.
    pub document_symbols: std::time::Duration,
    /// `textDocument/semanticTokens` (full).
    pub semantic_tokens: std::time::Duration,
    /// `textDocument/inlayHint`.
    pub inlay_hints: std::time::Duration,
    /// `textDocument/codeAction`.
    pub code_action: std::time::Duration,
    /// `textDocument/rename` — workspace-wide edit; matches the
    /// references budget.
    pub rename: std::time::Duration,
}

impl Default for LspMethodTimeoutsConfig {
    fn default() -> Self {
        Self {
            hover: std::time::Duration::from_millis(500),
            goto_definition: std::time::Duration::from_millis(500),
            completion: std::time::Duration::from_millis(1000),
            references: std::time::Duration::from_millis(2000),
            diagnostics: std::time::Duration::from_millis(5000),
            document_symbols: std::time::Duration::from_millis(1000),
            semantic_tokens: std::time::Duration::from_millis(1000),
            inlay_hints: std::time::Duration::from_millis(1000),
            code_action: std::time::Duration::from_millis(500),
            rename: std::time::Duration::from_millis(5000),
        }
    }
}

impl Default for EvictionPolicyConfig {
    fn default() -> Self {
        Self {
            // D119 — never trigger the LRU floor in default builds.
            memory_pressure_threshold: usize::MAX,
            // D40 — minimum live entries preserved when the LRU floor
            // runs. The 1024 default mirrors the soft cap observed
            // for typical project sizes in the corpus baseline; the
            // actual production tuning lives outside the plan scope.
            min_floor: 1024,
            // R22 — keep the most recent 3 `content_hash` variants
            // per canonical. Tracks the {current, previous,
            // baseline} window typical of an interactive editor.
            per_canonical_content_hash_retention: 3,
            // Promote an entry to "hot" after 2 warm hits; cold
            // entries age out first under memory pressure
            // regardless of `last_access` recency.
            promote_threshold: 2,
        }
    }
}

/// Configuration validation errors surfaced by
/// [`HostConfig::validate`].
#[derive(Debug, Clone, Eq, PartialEq, thiserror::Error)]
pub enum HostConfigError {
    /// `footprint_capture` is on but `audit_enabled` is off — the
    /// accumulator that backs footprint capture is created on the
    /// audit path and is inert without it.
    #[error("footprint_capture requires audit_enabled; enable both or neither")]
    FootprintCaptureWithoutAudit,
    /// `audit_timing_capture` is on but `audit_enabled` is off —
    /// the peak-RSS sampler and per-file timing helpers are gated
    /// behind the audit envelope and have no consumer when the
    /// envelope is disabled.
    #[error("audit_timing_capture requires audit_enabled; enable both or neither")]
    TimingCaptureWithoutAudit,
}

impl HostConfig {
    /// Create a config from a query profile.
    ///
    /// Sets `analysis_scope` from the profile's recommended scope mapping.
    /// This is the preferred migration path from AnalysisScope to QueryProfile.
    pub fn from_query_profile(profile: verter_semantic::profile::QueryProfile) -> Self {
        let scope_bits = profile.recommended_analysis_scope_bits();
        let scope = verter_semantic::analysis::AnalysisScope::from_bits_truncate(scope_bits);
        Self {
            analysis_scope: Some(scope),
            ..Default::default()
        }
    }
}

impl Default for HostConfig {
    fn default() -> Self {
        Self {
            dev_mode: true,
            compile_error_policy: CompileErrorPolicy::DevServeLastKnownGood,
            lsp_scheme: "verter-virtual".to_string(),
            max_profiles_per_file: 8,
            resolve_extensions: vec![
                ".ts".to_string(),
                ".tsx".to_string(),
                ".js".to_string(),
                ".jsx".to_string(),
                ".mts".to_string(),
                ".mjs".to_string(),
                ".cts".to_string(),
                ".cjs".to_string(),
                ".d.ts".to_string(),
                ".d.mts".to_string(),
                ".d.cts".to_string(),
            ],
            analysis_level: AnalysisLevel::Full,
            analysis_scope: None,
            generic_root_propagation: false,
            audit_enabled: false,
            footprint_capture: false,
            audit_timing_capture: false,
            max_derivation_edges: 10_000,
            depth_budget: crate::component_meta_materialize::MAX_DEPTH,
            projection_op_budget: 2000,
            eviction_policy: EvictionPolicyConfig::default(),
            lsp_method_timeouts: LspMethodTimeoutsConfig::default(),
            recursion_budget_overrides: RecursionBudgetOverrides::default(),
            typeinfo_scratch_cache_capacity: None,
        }
    }
}

impl HostConfig {
    /// Returns the effective analysis scope, preferring `analysis_scope`
    /// over the legacy `analysis_level` field.
    pub fn effective_scope(&self) -> verter_semantic::analysis::AnalysisScope {
        self.analysis_scope
            .unwrap_or_else(|| self.analysis_level.to_scope())
    }

    /// Validate cross-field invariants:
    ///
    /// - `footprint_capture` requires `audit_enabled` (the accumulator
    ///   is attached to the audit builder and is inert without it).
    /// - `audit_timing_capture` requires `audit_enabled` (the
    ///   per-host peak-RSS sampler and per-file timing helpers are
    ///   gated by the audit envelope).
    pub fn validate(&self) -> Result<(), HostConfigError> {
        if self.footprint_capture && !self.audit_enabled {
            return Err(HostConfigError::FootprintCaptureWithoutAudit);
        }
        if self.audit_timing_capture && !self.audit_enabled {
            return Err(HostConfigError::TimingCaptureWithoutAudit);
        }
        Ok(())
    }
}

/// Re-export `ProjectionMode` as the public resolver-mode API. Three of
/// the four variants cross the FFI boundary: `Identity`, `Shallow`,
/// `Expanded`. `Navigate` is dispatch-internal and must not be used at
/// the consumer/FFI surface.
pub use crate::semantic_query::ProjectionMode;

/// Per-compilation variant options.
///
/// A single `.vue` file can be compiled multiple times with different profiles
/// (e.g. client vs. SSR, dev vs. production). Each profile produces a separate
/// compile slot in the cache, keyed by the hash of this struct.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CompileProfile {
    /// Override filename passed to `verter_compiler` codegen (defaults to canonical ID).
    pub filename: Option<String>,
    /// Production mode: strips dev-only code (`__file`, HMR).
    pub is_production: bool,
    /// Server-side rendering mode.
    pub ssr: bool,
    /// HMR code injection strategy.
    pub hmr_strategy: HmrStrategy,
    /// Explicit component ID for scoped style hashing (auto-generated if `None`).
    pub component_id: Option<String>,
    /// Custom template expression delimiters (default `{{ }}`).
    pub delimiters: Option<(String, String)>,
    /// Tag names treated as custom elements (not resolved as Vue components).
    pub custom_elements: Option<Vec<String>>,
    /// Whether to preserve HTML comments in template output.
    pub comments: Option<bool>,
    /// Runtime module name for template imports (default `"vue"`).
    pub runtime_module_name: Option<String>,
    /// Types module name for TSX helper imports (default `"$verter/types"`).
    pub types_module_name: Option<String>,
    /// Force Vapor mode codegen regardless of `<template vapor>` attribute.
    pub force_vapor: bool,
    /// Strip TypeScript type annotations from compiled output.
    pub force_js: bool,
    /// Generate source maps for compiled output.
    pub source_map: bool,
    /// Controls which compilation steps run.
    /// See [`verter_compiler::compile::CompileTarget`] for available flags and presets.
    pub target: verter_compiler::compile::CompileTarget,
    /// Embed `declare module "@verter/types"` in generated TSX.
    /// When `false` (default), relies on the real `@verter/types` package.
    pub embed_ambient_types: bool,
    /// Experimental: Enable conditional root generic narrowing.
    pub conditional_root_narrowing: bool,
    /// Experimental: strict slot children type checking.
    pub strict_slots: bool,
}

impl Default for CompileProfile {
    fn default() -> Self {
        Self {
            filename: None,
            is_production: false,
            ssr: false,
            hmr_strategy: HmrStrategy::None,
            component_id: None,
            delimiters: None,
            custom_elements: None,
            comments: None,
            runtime_module_name: Some("vue".to_string()),
            types_module_name: None,
            force_vapor: false,
            force_js: false,
            source_map: false,
            target: verter_compiler::compile::CompileTarget::BUNDLER,
            embed_ambient_types: false,
            conditional_root_narrowing: false,
            strict_slots: false,
        }
    }
}

/// Discriminator for the virtual files produced from a single `.vue` SFC.
///
/// Each SFC is split into multiple virtual nodes that the bundler loads
/// independently (script, template, styles, custom blocks), plus a `Main`
/// node that assembles them into a single ES module.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum VirtualNodeKind {
    /// The assembled main module (imports styles, binds render function, exports component).
    Main,
    /// The `<script>` / `<script setup>` options object.
    Script,
    /// The compiled `<template>` render function.
    Template,
    /// A `<style>` block at the given index.
    Style { index: usize },
    /// A custom block (e.g. `<i18n>`) at the given index.
    Custom { index: usize },
}

/// A `src="..."` attribute on an SFC block that references an external file.
///
/// Produced during parsing when `<script src="...">`, `<template src="...">`,
/// or `<style src="...">` is encountered. The host uses these to fetch and
/// merge external content before compilation.
#[derive(Debug, Clone)]
pub struct ExternalSourceRequest {
    /// Canonical ID of the SFC that contains the `src` attribute.
    pub owner_canonical_id: String,
    /// Which block kind the `src` belongs to.
    pub block_kind: ExternalBlockKind,
    /// Block index (relevant for styles and custom blocks).
    pub index: usize,
    /// Raw specifier from the `src` attribute value.
    pub specifier: String,
    /// Resolved canonical path of the external file.
    pub resolved_canonical_id: String,
}

/// Which SFC block kind an external `src` reference belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalBlockKind {
    Script,
    Template,
    Style,
    Custom,
}

/// Granular description of which SFC slices changed between two file versions.
///
/// Used by the host to selectively invalidate compile slots and by the
/// caller (bundler/LSP) to trigger targeted HMR updates.
#[derive(Debug, Clone, Default)]
pub struct SliceChanges {
    /// The combined script block content hash changed.
    pub script_changed: bool,
    /// The template content hash changed.
    pub template_changed: bool,
    /// Indices of style blocks whose content hash changed.
    pub style_indices_changed: Vec<usize>,
    /// Indices of custom blocks whose content hash changed.
    pub custom_indices_changed: Vec<usize>,
    /// The number of blocks changed (script/template/style/custom count mismatch).
    pub structure_changed: bool,
    /// Block attributes changed (e.g. `lang`, `scoped`, `setup` added/removed).
    pub descriptor_changed: bool,
}

impl SliceChanges {
    /// Returns `true` when the only change is in style blocks — no script, template,
    /// structure, or descriptor changes. Useful for skipping TSGO sync since the
    /// generated TSX is unaffected by style-only edits.
    pub fn is_style_only(&self) -> bool {
        !self.script_changed
            && !self.template_changed
            && !self.structure_changed
            && !self.descriptor_changed
            && !self.style_indices_changed.is_empty()
    }
}

/// Summary of a single import statement found in a script block.
///
/// Returned in [`HostUpdateResult::import_specifiers`] so the caller can
/// resolve non-relative import paths via bundler/LSP resolution.
#[derive(Debug, Clone)]
pub struct ScriptImportInfo {
    /// The import source specifier (e.g. `"vue"`, `"./types"`).
    pub source: String,
    /// Whether this is a `import type` (type-only import).
    pub is_type_only: bool,
    /// Imported binding names.
    pub bindings: Vec<String>,
}

/// Summary of a single module reference found in a script block.
#[derive(Debug, Clone)]
pub struct ScriptModuleReference {
    /// Syntax form that introduced the reference.
    pub syntax: verter_semantic::analysis::ModuleReferenceSyntax,
    /// Import vs require semantics.
    pub semantics: verter_semantic::analysis::ModuleReferenceSemantics,
    /// Whether the site is declaration-level type-only.
    pub is_type_only: bool,
    /// Raw source text for the specifier expression.
    pub raw_text: String,
    /// Exact literal when statically known.
    pub literal_specifier: Option<String>,
    /// Finite set of literals when narrowed to a union.
    pub finite_specifiers: Vec<String>,
    /// Static prefix for dynamic expressions, if any.
    pub static_prefix: Option<String>,
    /// Static analyzability classification.
    pub analyzability: verter_semantic::analysis::ModuleReferenceAnalyzability,
    /// Span of the containing statement or call expression.
    pub span: verter_span::Span,
    /// Span of the specifier expression.
    pub expr_span: verter_span::Span,
}

/// Result of [`VerterHost::upsert`](crate::VerterHost::upsert) or
/// [`VerterHost::apply_style_overrides`](crate::VerterHost::apply_style_overrides).
///
/// Describes what changed so the caller can trigger targeted HMR updates
/// and resolve any external dependencies.
#[derive(Debug, Clone)]
#[must_use]
pub struct HostUpdateResult {
    /// Canonical file ID that was upserted.
    pub canonical_id: String,
    /// Whether any virtual nodes changed or were removed.
    pub changed: bool,
    /// Granular slice-level change breakdown.
    pub slice_changes: SliceChanges,
    /// Virtual node kinds that need recompilation.
    pub changed_virtual_nodes: Vec<VirtualNodeKind>,
    /// Virtual node kinds that were removed (e.g. a style block was deleted).
    pub removed_virtual_nodes: Vec<VirtualNodeKind>,
    /// Bundler-format virtual IDs for changed nodes.
    pub changed_virtual_ids: Vec<String>,
    /// Bundler-format virtual IDs for removed nodes.
    pub removed_virtual_ids: Vec<String>,
    /// LSP-format virtual IDs for changed nodes.
    pub changed_lsp_ids: Vec<String>,
    /// LSP-format virtual IDs for removed nodes.
    pub removed_lsp_ids: Vec<String>,
    /// Parse-phase diagnostics (syntax errors, warnings).
    pub diagnostics: DiagnosticsSnapshot,
    /// External `src="..."` requests that need caller-side file resolution.
    pub external_source_requests: Vec<ExternalSourceRequest>,
    /// Import specifiers found in script blocks, for caller-side resolution.
    pub import_specifiers: Vec<ScriptImportInfo>,
    /// Module reference sites found in script blocks.
    pub module_references: Vec<ScriptModuleReference>,
    /// Blocks that need external preprocessing before compilation.
    ///
    /// Non-empty when the SFC uses non-native languages (e.g., `<template lang="pug">`).
    /// The caller should invoke the appropriate preprocessor and feed results back
    /// via [`VerterHost::apply_block_overrides`](crate::VerterHost::apply_block_overrides).
    pub preprocessor_requests: Vec<PreprocessorRequest>,
    /// Export signatures extracted from the file's script block.
    /// For `.ts`/`.js` files these include re-export metadata for barrel file resolution.
    pub export_signatures: Vec<verter_semantic::analysis::ExportSignature>,
    /// Time spent in the parse phase (ms).
    pub parse_duration_ms: f64,
}

impl HostUpdateResult {
    /// Construct a no-op result for superseded upserts (scheduler mode).
    pub fn noop() -> Self {
        Self::no_change(String::new())
    }

    /// Construct a "nothing changed" result with all-empty change lists.
    pub fn no_change(canonical_id: String) -> Self {
        Self {
            canonical_id,
            changed: false,
            slice_changes: SliceChanges::default(),
            changed_virtual_nodes: Vec::new(),
            removed_virtual_nodes: Vec::new(),
            changed_virtual_ids: Vec::new(),
            removed_virtual_ids: Vec::new(),
            changed_lsp_ids: Vec::new(),
            removed_lsp_ids: Vec::new(),
            diagnostics: DiagnosticsSnapshot::default(),
            external_source_requests: Vec::new(),
            import_specifiers: Vec::new(),
            module_references: Vec::new(),
            preprocessor_requests: Vec::new(),
            export_signatures: Vec::new(),
            parse_duration_ms: 0.0,
        }
    }
}

/// Serializable snapshot of file analysis data, suitable for WASM export.
///
/// Returned by [`VerterHost::get_analysis`](crate::VerterHost::get_analysis).
/// Contains the combined script, style, and template analysis for an SFC.
///
/// Most fields are `Arc`-wrapped for cheap cloning — the underlying data is
/// shared between all snapshots of the same file version. Only `imports` and
/// `bindings` are owned `Vec`s because [`VerterHost::get_analysis`] mutates
/// them (import resolution and destructured binding enrichment).
#[derive(Debug, Clone, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileAnalysisSnapshot {
    /// Import statements found in script blocks.
    /// Owned because `resolve_snapshot_imports` mutates `resolved_canonical_id`.
    pub imports: Vec<verter_semantic::analysis::AnalyzedImport>,
    /// Module reference sites found in script blocks.
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub module_references: Arc<Vec<verter_semantic::analysis::AnalyzedModuleReference>>,
    /// Variable/function bindings declared in script blocks.
    /// Owned because `enrich_destructured_bindings` mutates `reactivity_kind`.
    pub bindings: Vec<verter_semantic::analysis::AnalyzedBinding>,
    /// Vue compiler macros used (defineProps, defineEmits, etc.).
    pub macros: Arc<Vec<verter_semantic::analysis::AnalyzedMacro>>,
    /// Type dependencies from macros that reference external files.
    pub macro_type_deps: Arc<Vec<verter_semantic::analysis::MacroTypeDep>>,
    /// Bitflags representing script characteristics (see `verter_semantic::analysis::ScriptFlags`).
    pub script_flags: u32,
    /// Per-style-block analysis (scoped, modules, v-bind usage).
    pub styles: Arc<Vec<verter_semantic::analysis::StyleBlockAnalysis>>,
    /// Template analysis (components, bindings, slots, refs, events).
    /// Present after compilation when template analysis scope flags are active.
    pub template: Option<Arc<verter_semantic::analysis::template::TemplateAnalysisSnapshot>>,
    /// Vue API call sites (lifecycle hooks, watchers, provide/inject, etc.).
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub vue_api_calls: Arc<Vec<verter_semantic::analysis::types::VueApiCallSite>>,
    /// DOM query call sites (querySelector, getElementById, etc.).
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub dom_query_calls: Arc<Vec<verter_semantic::analysis::types::DomQueryCallSite>>,

    /// CSS variable manipulations via DOM style APIs.
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub css_var_manipulations: Arc<Vec<verter_semantic::analysis::types::CssVarManipulation>>,

    /// Script-side binding usage occurrences with exact spans.
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub script_binding_occurrences:
        Arc<Vec<verter_semantic::analysis::types::ScriptBindingOccurrence>>,

    /// Export signatures extracted from the file's script block.
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub export_signatures: Arc<Vec<verter_semantic::analysis::ExportSignature>>,

    /// Options API analysis (`export default { ... }` or `export default defineComponent({ ... })`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options_api: Option<verter_semantic::analysis::AnalyzedOptionsApi>,

    /// Store usage sites (Pinia, Vuex, convention-based composables).
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub store_usages: Arc<Vec<verter_semantic::analysis::types::StoreUsage>>,
    /// Store definitions (defineStore, createStore, etc.).
    #[serde(default, skip_serializing_if = "arc_vec_is_empty")]
    pub store_definitions: Arc<Vec<verter_semantic::analysis::types::StoreDefinition>>,

    /// Whether the script block uses TypeScript (`lang="ts"`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub is_typescript: bool,
}

/// Compile-time dependencies that must be available before a Vue SFC can codegen.
#[derive(Debug, Clone, Default)]
pub struct CompileBlockersSnapshot {
    /// External `src="..."` blocks referenced by the SFC.
    pub external_source_requests: Vec<ExternalSourceRequest>,
    /// Macro type dependencies referenced from the SFC script.
    pub macro_type_deps: Arc<Vec<verter_semantic::analysis::MacroTypeDep>>,
}

/// A fully resolved export after following re-export chains.
///
/// Returned by [`VerterHost::resolve_exports`](crate::VerterHost::resolve_exports).
#[derive(Debug, Clone)]
pub struct ResolvedExport {
    /// Exported name as seen by importers.
    pub name: String,
    /// Whether this is a type-only export.
    pub is_type: bool,
    /// Ultimate source file canonical ID. `None` for local exports.
    pub source_canonical_id: Option<String>,
    /// Name in the ultimate source file (may differ, e.g. `"default"` → `"Button"`).
    pub source_name: String,
}

/// Helper for `skip_serializing_if` on `Arc<Vec<T>>`.
fn arc_vec_is_empty<T>(v: &Arc<Vec<T>>) -> bool {
    v.is_empty()
}

/// Result of [`VerterHost::resolve`](crate::VerterHost::resolve).
///
/// Maps a raw import identifier to its canonical ID and rendered virtual IDs
/// in both bundler query-string format and LSP `._VERTER_.` format.
#[derive(Debug, Clone)]
#[must_use]
pub struct ResolvedId {
    /// Normalized canonical file path (forward slashes, no query string).
    pub canonical_id: String,
    /// Which virtual node the raw ID points to.
    pub node_kind: VirtualNodeKind,
    /// Whether the file has been upserted into the host.
    pub exists_in_host: bool,
    /// Bundler-format virtual ID (e.g. `Comp.vue?vue&type=style&index=0&lang.css`).
    pub bundler_id: String,
    /// LSP-format virtual ID (e.g. `Comp.vue._VERTER_.style.0.css`).
    pub lsp_id: String,
}

/// Query parameters for [`VerterHost::get_virtual_file`](crate::VerterHost::get_virtual_file).
///
/// Provide either `raw_id` (parsed into canonical + node kind) or
/// `canonical_id` + `node_kind` explicitly. The `compile_profile` selects
/// which cached compile slot to use or compile with.
#[derive(Debug, Clone)]
pub struct VirtualQuery {
    /// Raw import ID (bundler or LSP format). Parsed to extract canonical ID and node kind.
    pub raw_id: Option<String>,
    /// Explicit canonical ID (used when `raw_id` is `None`).
    pub canonical_id: Option<String>,
    /// Explicit node kind (used when `raw_id` is `None`).
    pub node_kind: Option<VirtualNodeKind>,
    /// Compilation options for this request.
    pub compile_profile: CompileProfile,
}

/// Block-specific metadata attached to a [`VirtualFileResponse`].
#[derive(Debug, Clone, Default)]
pub struct VirtualMeta {
    /// Scoped style ID (e.g. `"data-v-abc123"`), present on Main nodes when styles are scoped.
    pub scope_id: Option<String>,
    /// Custom block type name (e.g. `"i18n"`).
    pub block_type: Option<String>,
    /// Style block index (for Style nodes).
    pub style_index: Option<usize>,
    /// Custom block index (for Custom nodes).
    pub custom_index: Option<usize>,
}

/// Result of [`VerterHost::get_virtual_file`](crate::VerterHost::get_virtual_file).
///
/// Contains the compiled code for a single virtual node, along with
/// diagnostics and metadata. The `stale` flag indicates fallback to
/// last-known-good output when the current source has compile errors.
#[derive(Debug, Clone)]
#[must_use]
pub struct VirtualFileResponse {
    /// Rendered virtual ID (bundler or LSP format, matching the query).
    pub id: String,
    /// Compiled code for this virtual node.
    pub code: Arc<str>,
    /// Source map (JSON string), if `source_map` was enabled in the profile.
    pub source_map: Option<Arc<str>>,
    /// Output language (e.g. `"js"`, `"ts"`, `"css"`, `"scss"`).
    pub lang: Option<String>,
    /// `true` if this is last-known-good fallback output (current source has errors).
    pub stale: bool,
    /// Compilation diagnostics (errors, warnings) from this compile slot.
    pub diagnostics: DiagnosticsSnapshot,
    /// Block-specific metadata (scope ID, block type, index).
    pub meta: VirtualMeta,
}

/// Input to [`VerterHost::upsert`](crate::VerterHost::upsert).
#[derive(Debug, Clone)]
pub struct UpsertRequest {
    /// Pre-resolved canonical ID. If `None`, derived from `input_id`.
    pub canonical_id: Option<String>,
    /// Raw file path or identifier as provided by the caller.
    pub input_id: String,
    /// Full source text of the file.
    pub source: Arc<str>,
    /// Whether this is a Vue SFC or a non-SFC dependency.
    pub file_kind: FileKind,
    /// Additional path aliases that should resolve to this file.
    pub aliases: Vec<String>,
}

/// Discriminates the SFC block type that needs external preprocessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreprocessorBlockType {
    Template,
    Script,
    Style,
    Custom,
}

/// A block that needs external preprocessing before compilation.
///
/// Returned in [`HostUpdateResult::preprocessor_requests`] when a block uses
/// a non-native `lang` attribute (e.g., `<template lang="pug">` or
/// `<script lang="coffee">`). The caller invokes the appropriate external
/// compiler and feeds the result back via
/// [`VerterHost::apply_block_overrides`](crate::VerterHost::apply_block_overrides).
#[derive(Debug, Clone)]
pub struct PreprocessorRequest {
    /// Which block type needs preprocessing.
    pub block_type: PreprocessorBlockType,
    /// Block index (0 for template/script, 0..N for styles/custom blocks).
    pub index: usize,
    /// The `lang` attribute value (e.g., `"pug"`, `"coffee"`, `"scss"`).
    pub lang: String,
    /// Raw content of the block that needs preprocessing.
    pub content: String,
}

/// A single preprocessed block override to inject into the compile pipeline.
///
/// Used in [`BlockOverrideRequest`] to provide the preprocessed output for
/// a template, script, style, or custom block.
#[derive(Debug, Clone)]
pub struct BlockOverrideEntry {
    /// Which block type this override applies to.
    pub block_type: PreprocessorBlockType,
    /// Block index (0 for template/script, 0..N for styles/custom blocks).
    pub index: usize,
    /// Preprocessed code (HTML for template, JS for script, CSS for style).
    pub code: Arc<str>,
    /// Source map from the preprocessor, if available.
    pub source_map: Option<Arc<str>>,
}

/// Input to [`VerterHost::apply_block_overrides`](crate::VerterHost::apply_block_overrides).
///
/// Unified API for applying preprocessed block overrides. Replaces the
/// deprecated [`StyleOverrideRequest`] for new code.
#[derive(Debug, Clone)]
pub struct BlockOverrideRequest {
    /// Canonical ID of the file whose blocks are being overridden.
    pub canonical_id: String,
    /// Compile profile to apply the overrides under.
    pub compile_profile: CompileProfile,
    /// Preprocessed block overrides to inject.
    pub overrides: Vec<BlockOverrideEntry>,
}

/// A single preprocessor-compiled style block to override in the compile cache.
#[derive(Debug, Clone)]
pub struct StyleOverrideEntry {
    /// Style block index this override applies to.
    pub index: usize,
    /// Preprocessed CSS code.
    pub code: Arc<str>,
    /// Source map from the preprocessor, if available.
    pub source_map: Option<Arc<str>>,
}

/// Input to [`VerterHost::apply_style_overrides`](crate::VerterHost::apply_style_overrides).
#[derive(Debug, Clone)]
pub struct StyleOverrideRequest {
    /// Canonical ID of the file whose styles are being overridden.
    pub canonical_id: String,
    /// Compile profile to apply the overrides under.
    pub compile_profile: CompileProfile,
    /// Preprocessed style blocks to inject.
    pub overrides: Vec<StyleOverrideEntry>,
}

/// Result of [`VerterHost::remove`](crate::VerterHost::remove).
#[derive(Debug, Clone)]
pub struct HostRemoveResult {
    /// Canonical ID of the removed file.
    pub canonical_id: String,
}

/// Severity level for a [`HostDiagnostic`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostSeverity {
    Error,
    Warning,
    Info,
}

/// A single diagnostic (error, warning, or info) produced during parsing or compilation.
#[derive(Debug, Clone)]
pub struct HostDiagnostic {
    pub severity: HostSeverity,
    /// Machine-readable error code (e.g. `"HOST_MISSING_EXTERNAL_SOURCE"`).
    pub code: String,
    /// Human-readable diagnostic message.
    pub message: String,
    /// SFC-absolute byte offset span, if available.
    pub span: Option<verter_span::Span>,
}

/// Collection of diagnostics with a precomputed `has_errors` flag.
#[derive(Debug, Clone, Default)]
pub struct DiagnosticsSnapshot {
    /// All diagnostics (errors + warnings + info).
    pub diagnostics: Vec<HostDiagnostic>,
    /// `true` if at least one diagnostic has [`HostSeverity::Error`].
    pub has_errors: bool,
}

impl DiagnosticsSnapshot {
    pub(crate) fn from_vec(diagnostics: Vec<HostDiagnostic>) -> Self {
        let has_errors = diagnostics
            .iter()
            .any(|d| d.severity == HostSeverity::Error);
        Self {
            diagnostics,
            has_errors,
        }
    }

    pub(crate) fn merge(mut self, mut other: DiagnosticsSnapshot) -> Self {
        self.diagnostics.append(&mut other.diagnostics);
        self.has_errors = self.has_errors || other.has_errors;
        self
    }
}

/// Errors returned by [`VerterHost`](crate::VerterHost) operations.
#[derive(Debug, Error)]
pub enum HostError {
    /// The requested file has not been upserted into the host.
    #[error("missing source for canonical id '{canonical_id}'")]
    MissingSource { canonical_id: String },
    /// The virtual query could not be parsed (neither `raw_id` nor `canonical_id` + `node_kind`).
    #[error("invalid virtual query")]
    InvalidQuery,
    /// The requested virtual node does not exist for this file.
    #[error("missing virtual node for id '{canonical_id}'")]
    MissingVirtualNode { canonical_id: String },
    /// Compilation failed. Contains the error diagnostics.
    #[error("compile error")]
    CompileError { diagnostics: DiagnosticsSnapshot },
    /// A scheduler error occurred.
    #[error("scheduler error: {0}")]
    Scheduler(#[from] verter_scheduler::job::SchedulerError),
    /// The request was superseded by a newer version of the file.
    #[error("request superseded by newer generation")]
    Superseded,
    /// The scheduler was shut down.
    #[error("scheduler shut down")]
    Shutdown,
}

#[derive(Debug, Clone)]
pub(crate) struct ParsedRawId {
    pub(crate) canonical_id: String,
    pub(crate) node_kind: VirtualNodeKind,
    pub(crate) was_lsp_like: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DescriptorMin {
    pub(crate) script_count: usize,
    pub(crate) template_count: usize,
    pub(crate) style_count: usize,
    pub(crate) custom_count: usize,
    pub(crate) script_attr_fingerprints: Vec<String>,
    pub(crate) template_attr_fingerprints: Vec<String>,
    pub(crate) style_attr_fingerprints: Vec<String>,
    pub(crate) custom_attr_fingerprints: Vec<String>,
    pub(crate) vapor: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct SliceHashes {
    pub(crate) script: Option<Hash16>,
    pub(crate) template: Option<Hash16>,
    pub(crate) styles: Vec<Hash16>,
    pub(crate) custom: Vec<Hash16>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FileMeta {
    pub(crate) has_script: bool,
    pub(crate) has_template: bool,
    /// True when any `<style scoped>` block exists. Used to expose a
    /// synthetic Script virtual node for template-only components that
    /// need `__scopeId` on the component object.
    pub(crate) has_scoped_style: bool,
    pub(crate) script_lang: Option<String>,
    /// Template lang attribute value (e.g., `"pug"`). `None` for native HTML.
    /// Stored for preprocessor request generation; read in tests.
    #[allow(dead_code)]
    pub(crate) template_lang: Option<String>,
    pub(crate) style_langs: Vec<Option<String>>,
    pub(crate) custom_types: Vec<String>,
    pub(crate) custom_langs: Vec<Option<String>>,
}

impl FileMeta {
    pub(crate) fn virtual_nodes(&self) -> Vec<VirtualNodeKind> {
        let mut nodes = vec![VirtualNodeKind::Main];
        if self.has_script || self.has_scoped_style {
            // Include Script for template-only components with scoped styles:
            // the compiler emits a synthetic script block with __scopeId.
            nodes.push(VirtualNodeKind::Script);
        }
        if self.has_template {
            nodes.push(VirtualNodeKind::Template);
        }
        for i in 0..self.style_langs.len() {
            nodes.push(VirtualNodeKind::Style { index: i });
        }
        for i in 0..self.custom_types.len() {
            nodes.push(VirtualNodeKind::Custom { index: i });
        }
        nodes
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SrcBlockInfo {
    pub(crate) tag_name: String,
    pub(crate) resolved_canonical_id: String,
    pub(crate) tag_open_start: u32,
    pub(crate) tag_open_end: u32,
    pub(crate) tag_close_start: Option<u32>,
}

#[derive(Debug, Clone)]
pub(crate) struct ParseSnapshot {
    pub(crate) whole_hash: Hash16,
    pub(crate) semantic_hash: Hash16,
    pub(crate) slices: SliceHashes,
    pub(crate) descriptor: DescriptorMin,
    pub(crate) meta: FileMeta,
    pub(crate) external_requests: Vec<ExternalSourceRequest>,
    pub(crate) src_blocks: Vec<SrcBlockInfo>,
    pub(crate) parse_diagnostics: DiagnosticsSnapshot,
    pub(crate) script_analysis: verter_semantic::analysis::ScriptAnalysisSnapshot,
    pub(crate) export_signatures: Vec<verter_semantic::analysis::ExportSignature>,
    pub(crate) style_analyses: Vec<verter_semantic::analysis::StyleBlockAnalysis>,
    /// Blocks that need external preprocessing (non-native `lang` attributes).
    pub(crate) preprocessor_requests: Vec<PreprocessorRequest>,
}

#[derive(Debug, Clone)]
pub(crate) struct StyleOverrideLayer {
    pub(crate) hash: u64,
    pub(crate) by_index: FxHashMap<usize, StyleOverrideEntry>,
}

/// Preprocessed template/script content that replaces the original block
/// content before compilation. Stored per compile-profile.
#[derive(Debug, Clone)]
pub(crate) struct ContentOverride {
    pub(crate) code: Arc<str>,
    pub(crate) source_map: Option<Arc<str>>,
}

/// Per-profile layer of content overrides for template and/or script blocks.
/// The `template` and `script` fields store the preprocessor output for future
/// source map chain support; currently only `hash` is read by the compile pipeline.
#[derive(Debug, Clone)]
pub(crate) struct ContentOverrideLayer {
    pub(crate) hash: u64,
    #[allow(dead_code)]
    pub(crate) template: Option<ContentOverride>,
    #[allow(dead_code)]
    pub(crate) script: Option<ContentOverride>,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedVirtualFile {
    pub(crate) code: Arc<str>,
    pub(crate) source_map: Option<Arc<str>>,
    pub(crate) lang: Option<String>,
    pub(crate) meta: VirtualMeta,
}

/// Cached IDE output for LSP type checking, stored separately from virtual files.
#[derive(Debug, Clone)]
pub(crate) struct CachedTsx {
    pub(crate) code: Arc<str>,
    pub(crate) source_map: Option<Arc<str>>,
    pub(crate) is_jsx: bool,
    pub(crate) destructured_block: Option<verter_compiler::compile::types::DestructuredBlockMeta>,
}

/// Response from [`VerterHost::get_ide`].
#[derive(Debug, Clone)]
pub struct IdeResponse {
    /// The generated TSX/JSX code.
    pub code: Arc<str>,
    /// JSON source map (if available).
    pub source_map: Option<Arc<str>>,
    /// `true` for JavaScript SFCs (.jsx output), `false` for TypeScript (.tsx output).
    pub is_jsx: bool,
    /// Structured metadata for the destructured block region, if present.
    pub destructured_block: Option<verter_compiler::compile::types::DestructuredBlockMeta>,
}

/// Response from [`VerterHost::get_public_api`].
///
/// Contains minimal TypeScript declarations generated by macro-only extraction
/// (defineProps, defineEmits, defineModel, defineOptions). No template
/// compilation is performed.
#[derive(Debug, Clone)]
pub struct TscResponse {
    /// The generated TSC code (ComponentPublicInstance-based declaration).
    /// Includes an inline `//# sourceMappingURL=` at the end.
    pub code: Arc<str>,
    /// JSON source map (always present — embedded inline in `code`).
    pub source_map: Option<Arc<str>>,
}

/// Controls which public API surface the host generates for a Vue SFC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicApiMode {
    /// The normal public instance surface used by application code.
    Public,
    /// A Vue Test Utils-like debug surface that exposes `<script setup>` bindings.
    Testing,
}

#[derive(Debug, Clone)]
pub(crate) struct CompileSlot {
    pub(crate) semantic_hash: Hash16,
    pub(crate) style_override_hash: u64,
    pub(crate) content_override_hash: u64,
    pub(crate) outputs: FxHashMap<VirtualNodeKind, CachedVirtualFile>,
    pub(crate) diagnostics: DiagnosticsSnapshot,
    pub(crate) last_good_outputs: Option<FxHashMap<VirtualNodeKind, CachedVirtualFile>>,
    #[allow(dead_code)]
    pub(crate) last_access_tick: u64,
    /// Combined TSX output for LSP type checking. Not a virtual file.
    pub(crate) tsx: Option<CachedTsx>,
    /// Template analysis extracted during compilation. Populated when
    /// the analysis scope includes template flags (TPL_COMPONENTS, etc.).
    /// Stored per-slot for future per-profile access; the latest is also
    /// copied to `FileEntry::template_analysis` for the public API.
    #[allow(dead_code)]
    pub(crate) template_analysis:
        Option<verter_semantic::analysis::template::TemplateAnalysisSnapshot>,
}

/// Lightweight extract of FileEntry fields needed for compilation,
/// avoids cloning heavy compile_slots and style_overrides maps.
pub(crate) struct CompileInput {
    pub(crate) canonical_id: String,
    pub(crate) source: Arc<str>,
    pub(crate) meta: FileMeta,
    pub(crate) parse_diagnostics: DiagnosticsSnapshot,
    pub(crate) src_blocks: Vec<SrcBlockInfo>,
    pub(crate) external_requests: Vec<ExternalSourceRequest>,
    pub(crate) style_override_layer: Option<StyleOverrideLayer>,
    /// Content overrides for preprocessed template/script blocks.
    pub(crate) content_override_layer: Option<ContentOverrideLayer>,
    /// Macro type dependencies for cross-file type resolution.
    pub(crate) macro_type_deps: Vec<verter_semantic::analysis::MacroTypeDep>,
    /// Import declarations from the SFC script analysis.
    /// Used to attach precise spans to unresolved compile blockers.
    pub(crate) script_imports: Vec<verter_semantic::analysis::AnalyzedImport>,
    /// Macro calls from the effective script analysis.
    /// Used when converting template compiler metadata into host analysis.
    pub(crate) script_macros: Vec<verter_semantic::analysis::AnalyzedMacro>,
    /// Local/exported bindings from the effective script analysis.
    /// Used when converting template compiler metadata into host analysis.
    pub(crate) script_bindings: Vec<verter_semantic::analysis::AnalyzedBinding>,
    /// Cached parsed SFC from upsert, reused during compilation to avoid re-parsing.
    pub(crate) cached_parse: Option<Arc<verter_compiler::parser::types::ParsedSfc>>,
    /// Binding names referenced in style `v-bind()` expressions.
    /// Extracted from `FileEntry.style_analyses` at cache-miss time.
    pub(crate) style_v_bind_vars: Vec<String>,
}

/// Cached Arc-wrapped views of immutable `ScriptAnalysisSnapshot` fields.
///
/// Built once during upsert and shared across all `get_analysis()` calls.
/// Per-specifier resolution record for an import dependency.
///
/// Callers (unplugin, LSP, TS plugin) resolve import specifiers to canonical IDs
/// and pass these records to [`VerterHost::set_import_dependencies`]. The host uses
/// them for exact resolution instead of lossy basename/suffix heuristics.
#[derive(Debug, Clone)]
pub struct DependencyResolution {
    /// The raw import specifier as written in source (e.g., `@/components/base`).
    pub specifier: String,
    /// Exact resolved canonical ID, if the caller resolved it (e.g., `/src/components/base/index.ts`).
    pub resolved_canonical_id: Option<String>,
    /// Candidate canonical IDs when exact resolution isn't available.
    /// Selection uses TS-first priority via [`effective_target()`]: `.d.ts` > `.ts` >
    /// `.tsx` > `.js`. Only the single highest-priority candidate is used; remaining
    /// candidates are not tried if the selected one lacks the needed type.
    pub possible_canonical_ids: Vec<String>,
}

/// TS-first extension priority for unresolved candidate selection.
///
/// Verter relies on TS typing for type-strict analysis. JS files are
/// fallback-only when no TS type definition exists. Lower value = higher
/// priority.
pub(crate) fn extension_priority(path: &str) -> u8 {
    if path.ends_with(".d.ts") {
        0
    } else if path.ends_with(".d.cts") {
        1
    } else if path.ends_with(".d.mts") {
        2
    } else if path.ends_with(".ts") {
        3
    } else if path.ends_with(".tsx") {
        4
    } else if path.ends_with(".js") {
        5
    } else if path.ends_with(".jsx") {
        6
    } else if path.ends_with(".cjs") {
        7
    } else if path.ends_with(".mjs") {
        8
    } else {
        // Non-script files (.vue, .json, .css) — only selected when
        // no script candidates exist.
        9
    }
}

impl DependencyResolution {
    /// Returns the single effective canonical ID for this resolution.
    ///
    /// When `resolved_canonical_id` is present, returns that directly.
    /// Otherwise picks the single highest-priority candidate using TS-first
    /// ordering. If the selected candidate does not contain the needed type,
    /// callers should treat the resolution as not found — do NOT try
    /// remaining candidates.
    pub fn effective_target(&self) -> Option<&str> {
        if let Some(ref id) = self.resolved_canonical_id {
            return Some(id.as_str());
        }
        self.possible_canonical_ids
            .iter()
            .min_by_key(|c| extension_priority(c))
            .map(|s| s.as_str())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// CompileCacheEntry — scheduler-backed compile state (native only)
// ═══════════════════════════════════════════════════════════════════════════

/// Profile-domain state for the scheduler-backed compile cache (D48).
///
/// Stored in [`crate::project_type_store::CompileCacheDb`] keyed by canonical id.
/// Profile-flag changes invalidate this; source-content changes preserve it;
/// dep-closure changes preserve it; project-generation bumps invalidate it.
/// See the §3.4.2 invalidation matrix.
#[derive(Debug, Default)]
pub struct ProfileState {
    pub(crate) content_overrides: FxHashMap<u64, ContentOverrideWithParse>,
    pub(crate) style_overrides: FxHashMap<u64, StyleOverrideWithAnalysis>,
    pub(crate) compile_slots: FxHashMap<u64, CompileSlot>,
    pub(crate) latest_diagnostics: FxHashMap<u64, DiagnosticsSnapshot>,
    pub(crate) diagnostics_generation: u64,
}

/// Source-content-domain state for the scheduler-backed compile cache (D48).
///
/// Stored in [`crate::project_type_store::DerivedRawCacheDb`] keyed by canonical id.
/// Source-content changes invalidate this; profile-flag changes preserve it;
/// dep-closure changes preserve it; project-generation bumps invalidate it.
/// See the §3.4.2 invalidation matrix.
///
/// `import_routes` is a sub-mirror of
/// [`crate::project_type_store::IndexedReady`]`.import_routes`: same content,
/// different invalidation trigger. Source-content change for the owner drops
/// this DerivedRawState entry (along with the IndexedReady entry it mirrored);
/// profile-flag change preserves DerivedRawState while leaving IndexedReady
/// untouched. The asymmetry that motivated D48 is the per-domain trigger
/// independence — keeping `import_routes` here means a profile-flag sweep
/// no longer drops the resolved-route cache redundantly.
#[derive(Debug, Default)]
pub struct DerivedRawState {
    /// Sub-mirror of [`IndexedReady.import_routes`](crate::project_type_store::IndexedReady).
    /// Same content, different invalidation trigger from the IndexedReady source —
    /// see the §3.4.2 invalidation matrix on the rehoming doc. Source content change
    /// for the owner drops this; profile-flag change preserves it.
    pub(crate) import_routes: FxHashMap<String, DependencyResolution>,

    /// Cached TSC extract keyed by whole_hash. On read: stored hash must match
    /// `effective_file_state().whole_hash`. Cleared on upsert when whole_hash changes.
    pub(crate) cached_tsc_extract: Option<(Hash16, Arc<verter_compiler::tsc::ExtractedTscState>)>,
    /// Mode-aware cached resolved component-meta sidecar keyed by owner/dependency hashes.
    pub(crate) cached_resolved_meta: FxHashMap<ProjectionMode, ResolvedComponentMetaCacheEntry>,
    /// Cached encoded protobuf payload for the canonical component-meta query.
    pub(crate) cached_meta_payload: Option<CachedMetaPayload>,

    /// Raw template analysis (source-derived, profileless).
    /// Computed by `compute_template_analysis_if_missing()` from raw scheduler data.
    /// Always raw — never from overrides.
    ///
    /// EXTERNAL SRC RULE: When src_blocks is non-empty, raw_template_analysis is NOT cached
    /// (set to None after read). Editing an external `<template src>` / `<script src>` dep
    /// only triggers `smart_invalidate_dependents` (which clears compile_slots), not
    /// raw_template_analysis.
    pub(crate) raw_template_analysis:
        Option<Arc<verter_semantic::analysis::template::TemplateAnalysisSnapshot>>,

    /// Cached fallthrough resolution keyed by semantic fact versions and
    /// generic-root-propagation behavior. Cleared everywhere
    /// `cached_resolved_meta` is cleared.
    pub(crate) cached_fallthrough: Option<CachedFallthroughEntry>,

    /// Eviction flag — when true, the file is invisible to host accessors
    /// but deps/aliases (in [`DependencyState`]) are preserved for old-state
    /// diffing during reload.
    pub(crate) evicted: bool,

    /// Whole-hash recorded at eviction time, when available. `None` indicates
    /// the caller did not have the hash in scope (e.g. eviction triggered by
    /// a delete path that already discarded the snapshot). Read by
    /// `ensure_loaded` to short-circuit the `bump_store_view_epoch` when an
    /// evicted file reloads with identical content — preserves the type-context
    /// cache across no-op reloads. `None` triggers the conservative bump that
    /// matches pre-fix behavior.
    pub(crate) evicted_whole_hash: Option<Hash16>,
}

/// Dependency-closure-domain state for the scheduler-backed compile cache (D48).
///
/// Stored in [`crate::project_type_store::DependencyCacheDb`] keyed by canonical id.
/// Dep-closure changes invalidate this; source-content changes invalidate it
/// (because dep-closure is recomputed); profile-flag changes preserve it;
/// project-generation bumps invalidate it. See the §3.4.2 invalidation matrix.
#[derive(Debug, Default)]
pub struct DependencyState {
    pub(crate) dependencies: std::collections::BTreeSet<String>,
    /// Retired observability surface: per-dep type hashes formerly
    /// populated by the deleted smart-invalidation cascade. Field
    /// preserved for future affected-files reporting; no current
    /// reader.
    #[allow(dead_code)]
    pub(crate) resolved_type_hashes: FxHashMap<(String, String), Hash16>,
    pub(crate) aliases: std::collections::BTreeSet<String>,
    pub(crate) generation: u64,
}

/// Override-aware file state returned by `effective_file_state()`.
///
/// Contains either the raw scheduler data or the content override's synthetic
/// data, depending on whether a block override exists for the requested profile.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields read progressively as accessors migrate
pub(crate) struct EffectiveFileState {
    pub(crate) source: std::sync::Arc<str>,
    pub(crate) meta: FileMeta,
    pub(crate) script_analysis: verter_semantic::analysis::ScriptAnalysisSnapshot,
    pub(crate) cached_parse: Option<std::sync::Arc<verter_compiler::parser::types::ParsedSfc>>,
    pub(crate) whole_hash: Hash16,
}

/// Block override + cached re-parse from synthetic source.
///
/// When a preprocessor (e.g. Pug → HTML) overrides a block, the host builds a
/// synthetic SFC source, re-parses it, and stores the result here. The scheduler's
/// raw source/analysis are never modified by overrides.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Used in apply_block_overrides
pub(crate) struct ContentOverrideWithParse {
    pub(crate) layer: ContentOverrideLayer,
    pub(crate) parse: ParseSnapshot,
    pub(crate) cached_parse: Option<Arc<verter_compiler::parser::types::ParsedSfc>>,
    pub(crate) source: Arc<str>,
}

/// Style override + remapped CSS analyses + lang overrides.
///
/// When a style preprocessor (e.g. SCSS → CSS) runs, the compiled CSS and its
/// remapped CSS analysis (with SFC-absolute spans) are stored here per-profile.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Used in apply_style_overrides
pub(crate) struct StyleOverrideWithAnalysis {
    pub(crate) layer: StyleOverrideLayer,
    /// Per-index: Some(remapped CSS analysis) for overridden blocks, None for raw.
    pub(crate) analyses: Vec<Option<verter_semantic::analysis::StyleBlockAnalysis>>,
    /// Per-index: Some("css") for overridden blocks, None for raw.
    pub(crate) lang_overrides: Vec<Option<String>>,
    pub(crate) hash: u64,
}

// ═══════════════════════════════════════════════════════════════════════════
// ResolvedExternalTypeCache — host-level shared resolved type cache
// ═══════════════════════════════════════════════════════════════════════════

/// Maximum entries in the host-level resolved external type cache.
pub(crate) const RESOLVED_TYPE_CACHE_CAP: usize = 4096;

/// Maximum recursion depth for external type resolution.
///
/// Safety net for pathological barrel chains. The barrel resolution cache
/// and visiting set handle all practical cases; this limit only fires for
/// truly extreme input (e.g., 130+ nested `export *` chains).
pub(crate) const MAX_RESOLVE_DEPTH: usize = 128;

/// Maximum distinct `(canonical,type)` external-resolution pairs per request.
///
/// This is a hard safety cap for component-meta and macro type resolution.
/// When a single imported surface fans out into an unexpectedly large graph,
/// fail explicitly instead of continuing to allocate until the caller runs
/// out of memory.
pub(crate) const MAX_EXTERNAL_TYPE_RESOLVE_STEPS: usize = 2_000;

/// Error from [`VerterHost::resolve_external_type_from_loaded_files`].
#[derive(Debug, Clone)]
pub enum ExternalTypeResolveError {
    /// The root dependency could not be resolved.
    MissingRootDependency,
    /// Recursion depth exceeded the configured limit.
    DepthLimitExceeded {
        limit: usize,
        type_name: String,
        last_dep: String,
    },
    /// Total distinct external-type resolution steps exceeded the hard limit.
    StepLimitExceeded {
        limit: usize,
        type_name: String,
        last_dep: String,
    },
}

/// Key for the host-level resolved external type cache.
///
/// Includes the dependency's source hash to guarantee freshness — when a
/// dependency file changes, its hash changes and stale entries are never hit.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct ResolvedTypeCacheKey {
    pub dep_canonical_id: String,
    pub dep_source_hash: Hash16,
    pub type_name: String,
    pub resolve_kind: verter_workspace::ResolveRequestKind,
}

/// A resolved external type entry in the host-level cache.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedTypeCacheEntry {
    pub resolved: Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements>,
    /// Canonical IDs traversed during resolution. Replayed into the caller's
    /// `tracked_deps` on cache hit so the eval path knows which sources to read.
    pub tracked_deps: Vec<String>,
}

/// Cached host-owned component-meta resolved state.
///
/// The declaration-graph traversal cache remains shared and mode-agnostic.
/// This cache stores the mode-specific materialized sidecar and verifies that
/// both the owner file and every tracked dependency still match the hashes that
/// produced the cached state.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedComponentMetaCacheEntry {
    pub fact_versions: Vec<crate::resolver_core::FactVersionRef>,
    pub state: Arc<crate::meta_resolve::ResolvedComponentMetaState>,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedFallthroughEntry {
    pub fact_versions: Vec<crate::resolver_core::FactVersionRef>,
    pub generic_root_propagation: bool,
    pub resolution: Arc<FallthroughResolution>,
}

/// Cached encoded protobuf payload for a component-meta query.
#[derive(Debug, Clone)]
pub(crate) struct CachedMetaPayload {
    pub fact_versions: Vec<crate::resolver_core::FactVersionRef>,
    pub payload: Vec<u8>,
}

// ═══════════════════════════════════════════════════════════════════════════
// MetaProvenance — per-host counters for component-meta observability
// ═══════════════════════════════════════════════════════════════════════════

/// Number of [`crate::semantic_query::SemanticNodeData`] discriminants used
/// to size the per-discriminant push-count array in [`MetaProvenance`].
///
/// Sized with headroom over the current 20 variants so adding a
/// variant doesn't require widening the array. If
/// `SemanticNodeData::discriminant_index` ever returns `>= 24`, that's
/// a debug-assert hit at the push site rather than a silent overflow.
pub const SEMANTIC_NODE_DATA_DISCRIMINANT_COUNT: usize = 24;

/// Per-host provenance counters for component-meta observability.
///
/// AtomicU64 for thread-safe increment. Reset on host close. Not persisted.
/// Host tests read counters directly via `host.provenance()`.
///
/// The `ensure_loaded_*`, `execute_cooperative_*`, `overlay_gate_*`,
/// and `node_arena_*` families count cooperative-execute path
/// selection, intern hot-path activity, and lock hold/wait time so
/// future tuning passes (e.g., interner sharding) can be
/// evidence-driven.
pub struct MetaProvenance {
    pub get_component_meta_calls: std::sync::atomic::AtomicU64,
    pub component_meta_resolved_state_recomputes: std::sync::atomic::AtomicU64,
    pub get_analysis_calls: std::sync::atomic::AtomicU64,
    pub evaluate_types_calls: std::sync::atomic::AtomicU64,
    /// Bumped on every `VerterHost::upsert(...)` call. Used by
    /// `tests/session_view_isolation.rs` to assert the R17
    /// invariant that session query paths do NOT mutate the host.
    pub host_upsert_calls: std::sync::atomic::AtomicU64,
    /// Bumped on every cache-key derivation that consulted a
    /// [`crate::session_view::SessionView`] via
    /// `view.content_hash_for(canonical)` rather than the base host's
    /// `shallow_file_state(canonical).whole_hash`. Used by
    /// `tests/session_view_warm_reuse.rs` to assert R17/R18 (the
    /// consumer path is wired through `SessionView`, not through the
    /// bare host).
    pub view_aware_cache_key_lookups: std::sync::atomic::AtomicU64,
    pub resolved_external_type_cache_hits: std::sync::atomic::AtomicU64,
    pub resolved_external_type_cache_misses: std::sync::atomic::AtomicU64,
    pub resolver_node_cache_hits: std::sync::atomic::AtomicU64,
    pub resolver_node_cache_misses: std::sync::atomic::AtomicU64,
    pub resolver_singleflight_coalesced: std::sync::atomic::AtomicU64,
    pub resolver_cross_view_lane_forks: std::sync::atomic::AtomicU64,
    pub resolver_cycle_detections: std::sync::atomic::AtomicU64,
    pub resolver_route_fact_reuse: std::sync::atomic::AtomicU64,
    pub resolver_barrel_fact_reuse: std::sync::atomic::AtomicU64,
    pub import_resolution_cache_hit_count: std::sync::atomic::AtomicU64,
    pub import_resolution_cache_miss_count: std::sync::atomic::AtomicU64,
    pub dir_index_hit_count: std::sync::atomic::AtomicU64,
    pub dir_index_refresh_count: std::sync::atomic::AtomicU64,
    pub dir_index_dirty_rescan_count: std::sync::atomic::AtomicU64,
    pub native_fs_read_dir_count: std::sync::atomic::AtomicU64,
    pub native_fs_read_file_miss_count: std::sync::atomic::AtomicU64,
    pub payload_cache_hits: std::sync::atomic::AtomicU64,
    pub payload_cache_misses: std::sync::atomic::AtomicU64,
    pub payload_encodes: std::sync::atomic::AtomicU64,
    pub indexed_ready_scheduler_snapshot_reuse: std::sync::atomic::AtomicU64,
    pub bundle_cache_hits: std::sync::atomic::AtomicU64,
    pub bundle_materializations: std::sync::atomic::AtomicU64,
    pub dep_resolution_calls: std::sync::atomic::AtomicU64,
    pub imported_macro_declaration_builds: std::sync::atomic::AtomicU64,
    pub route_owned_snapshot_cache_hits: std::sync::atomic::AtomicU64,
    pub route_owned_snapshot_cached_parse_hits: std::sync::atomic::AtomicU64,

    // ── Path C C1 contention instrumentation ────────────────────────────
    /// `VerterHost::ensure_loaded` invocation count.
    pub ensure_loaded_calls: std::sync::atomic::AtomicU64,
    /// Time spent inside `Scheduler::wait_or_drive` from `ensure_loaded`.
    pub ensure_loaded_wait_ns: std::sync::atomic::AtomicU64,
    /// Time spent inside `integrate_scheduler_snapshot` from `ensure_loaded`.
    pub ensure_loaded_work_ns: std::sync::atomic::AtomicU64,
    /// `SemanticGraphStore::execute_cooperative` calls that became the cold
    /// owner (claimed in-flight slot).
    pub execute_cooperative_owner_path: std::sync::atomic::AtomicU64,
    /// `execute_cooperative` calls that joined an in-flight build.
    pub execute_cooperative_joiner_path: std::sync::atomic::AtomicU64,
    /// Time the cold owner held the in-flight slot (build duration).
    pub execute_cooperative_held_ns: std::sync::atomic::AtomicU64,
    /// `NodeArena::push_impl` total call count (every push, exempt or not).
    pub node_arena_pushes: std::sync::atomic::AtomicU64,
    /// `NodeArena::push_impl` calls that allocated a new arena slot
    /// (always equal to `node_arena_pushes` pre-C7, diverges once
    /// structural interning lands).
    pub node_arena_intern_miss: std::sync::atomic::AtomicU64,
    /// Time spent waiting on `ArenaInner` mutex acquisition during pushes
    /// (C17 observability per Pass C17).
    pub node_arena_inner_write_wait_ns: std::sync::atomic::AtomicU64,
    /// Scheduler submission count (mirrored from
    /// `verter_scheduler::scheduler::SchedulerCounters::submit_count` via
    /// `VerterHost::provenance_snapshot`). The direct-memoized field stays
    /// zero; `provenance_snapshot` overwrites it with the live value.
    pub scheduler_submit_count: std::sync::atomic::AtomicU64,
    /// Scheduler peak inbox depth (mirrored from `SchedulerCounters`).
    pub scheduler_inbox_depth_max: std::sync::atomic::AtomicU64,
    /// Per-`SemanticNodeData` discriminant push count, indexed by
    /// `SemanticNodeData::discriminant_index()`. Sized to
    /// [`SEMANTIC_NODE_DATA_DISCRIMINANT_COUNT`] for variant headroom.
    pub node_arena_pushes_per_discriminant:
        [std::sync::atomic::AtomicU64; SEMANTIC_NODE_DATA_DISCRIMINANT_COUNT],
}

impl Default for MetaProvenance {
    fn default() -> Self {
        Self {
            get_component_meta_calls: std::sync::atomic::AtomicU64::new(0),
            component_meta_resolved_state_recomputes: std::sync::atomic::AtomicU64::new(0),
            get_analysis_calls: std::sync::atomic::AtomicU64::new(0),
            evaluate_types_calls: std::sync::atomic::AtomicU64::new(0),
            host_upsert_calls: std::sync::atomic::AtomicU64::new(0),
            view_aware_cache_key_lookups: std::sync::atomic::AtomicU64::new(0),
            resolved_external_type_cache_hits: std::sync::atomic::AtomicU64::new(0),
            resolved_external_type_cache_misses: std::sync::atomic::AtomicU64::new(0),
            resolver_node_cache_hits: std::sync::atomic::AtomicU64::new(0),
            resolver_node_cache_misses: std::sync::atomic::AtomicU64::new(0),
            resolver_singleflight_coalesced: std::sync::atomic::AtomicU64::new(0),
            resolver_cross_view_lane_forks: std::sync::atomic::AtomicU64::new(0),
            resolver_cycle_detections: std::sync::atomic::AtomicU64::new(0),
            resolver_route_fact_reuse: std::sync::atomic::AtomicU64::new(0),
            resolver_barrel_fact_reuse: std::sync::atomic::AtomicU64::new(0),
            import_resolution_cache_hit_count: std::sync::atomic::AtomicU64::new(0),
            import_resolution_cache_miss_count: std::sync::atomic::AtomicU64::new(0),
            dir_index_hit_count: std::sync::atomic::AtomicU64::new(0),
            dir_index_refresh_count: std::sync::atomic::AtomicU64::new(0),
            dir_index_dirty_rescan_count: std::sync::atomic::AtomicU64::new(0),
            native_fs_read_dir_count: std::sync::atomic::AtomicU64::new(0),
            native_fs_read_file_miss_count: std::sync::atomic::AtomicU64::new(0),
            payload_cache_hits: std::sync::atomic::AtomicU64::new(0),
            payload_cache_misses: std::sync::atomic::AtomicU64::new(0),
            payload_encodes: std::sync::atomic::AtomicU64::new(0),
            indexed_ready_scheduler_snapshot_reuse: std::sync::atomic::AtomicU64::new(0),
            bundle_cache_hits: std::sync::atomic::AtomicU64::new(0),
            bundle_materializations: std::sync::atomic::AtomicU64::new(0),
            dep_resolution_calls: std::sync::atomic::AtomicU64::new(0),
            imported_macro_declaration_builds: std::sync::atomic::AtomicU64::new(0),
            route_owned_snapshot_cache_hits: std::sync::atomic::AtomicU64::new(0),
            route_owned_snapshot_cached_parse_hits: std::sync::atomic::AtomicU64::new(0),
            ensure_loaded_calls: std::sync::atomic::AtomicU64::new(0),
            ensure_loaded_wait_ns: std::sync::atomic::AtomicU64::new(0),
            ensure_loaded_work_ns: std::sync::atomic::AtomicU64::new(0),
            execute_cooperative_owner_path: std::sync::atomic::AtomicU64::new(0),
            execute_cooperative_joiner_path: std::sync::atomic::AtomicU64::new(0),
            execute_cooperative_held_ns: std::sync::atomic::AtomicU64::new(0),
            node_arena_pushes: std::sync::atomic::AtomicU64::new(0),
            node_arena_intern_miss: std::sync::atomic::AtomicU64::new(0),
            node_arena_inner_write_wait_ns: std::sync::atomic::AtomicU64::new(0),
            scheduler_submit_count: std::sync::atomic::AtomicU64::new(0),
            scheduler_inbox_depth_max: std::sync::atomic::AtomicU64::new(0),
            node_arena_pushes_per_discriminant: std::array::from_fn(|_| {
                std::sync::atomic::AtomicU64::new(0)
            }),
        }
    }
}

impl std::fmt::Debug for MetaProvenance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use std::sync::atomic::Ordering::Relaxed;
        f.debug_struct("MetaProvenance")
            .field(
                "get_component_meta_calls",
                &self.get_component_meta_calls.load(Relaxed),
            )
            .field(
                "component_meta_resolved_state_recomputes",
                &self.component_meta_resolved_state_recomputes.load(Relaxed),
            )
            .field("get_analysis_calls", &self.get_analysis_calls.load(Relaxed))
            .field(
                "evaluate_types_calls",
                &self.evaluate_types_calls.load(Relaxed),
            )
            .field(
                "resolved_external_type_cache_hits",
                &self.resolved_external_type_cache_hits.load(Relaxed),
            )
            .field(
                "resolved_external_type_cache_misses",
                &self.resolved_external_type_cache_misses.load(Relaxed),
            )
            .field(
                "resolver_node_cache_hits",
                &self.resolver_node_cache_hits.load(Relaxed),
            )
            .field(
                "resolver_node_cache_misses",
                &self.resolver_node_cache_misses.load(Relaxed),
            )
            .field(
                "resolver_singleflight_coalesced",
                &self.resolver_singleflight_coalesced.load(Relaxed),
            )
            .field(
                "resolver_cross_view_lane_forks",
                &self.resolver_cross_view_lane_forks.load(Relaxed),
            )
            .field(
                "resolver_cycle_detections",
                &self.resolver_cycle_detections.load(Relaxed),
            )
            .field(
                "resolver_route_fact_reuse",
                &self.resolver_route_fact_reuse.load(Relaxed),
            )
            .field(
                "resolver_barrel_fact_reuse",
                &self.resolver_barrel_fact_reuse.load(Relaxed),
            )
            .field(
                "import_resolution_cache_hit_count",
                &self.import_resolution_cache_hit_count.load(Relaxed),
            )
            .field(
                "import_resolution_cache_miss_count",
                &self.import_resolution_cache_miss_count.load(Relaxed),
            )
            .field(
                "dir_index_hit_count",
                &self.dir_index_hit_count.load(Relaxed),
            )
            .field(
                "dir_index_refresh_count",
                &self.dir_index_refresh_count.load(Relaxed),
            )
            .field(
                "dir_index_dirty_rescan_count",
                &self.dir_index_dirty_rescan_count.load(Relaxed),
            )
            .field(
                "native_fs_read_dir_count",
                &self.native_fs_read_dir_count.load(Relaxed),
            )
            .field(
                "native_fs_read_file_miss_count",
                &self.native_fs_read_file_miss_count.load(Relaxed),
            )
            .field("payload_cache_hits", &self.payload_cache_hits.load(Relaxed))
            .field(
                "payload_cache_misses",
                &self.payload_cache_misses.load(Relaxed),
            )
            .field("payload_encodes", &self.payload_encodes.load(Relaxed))
            .field(
                "indexed_ready_scheduler_snapshot_reuse",
                &self.indexed_ready_scheduler_snapshot_reuse.load(Relaxed),
            )
            .field("bundle_cache_hits", &self.bundle_cache_hits.load(Relaxed))
            .field(
                "bundle_materializations",
                &self.bundle_materializations.load(Relaxed),
            )
            .field(
                "dep_resolution_calls",
                &self.dep_resolution_calls.load(Relaxed),
            )
            .field(
                "imported_macro_declaration_builds",
                &self.imported_macro_declaration_builds.load(Relaxed),
            )
            .field(
                "route_owned_snapshot_cache_hits",
                &self.route_owned_snapshot_cache_hits.load(Relaxed),
            )
            .field(
                "route_owned_snapshot_cached_parse_hits",
                &self.route_owned_snapshot_cached_parse_hits.load(Relaxed),
            )
            .field(
                "ensure_loaded_calls",
                &self.ensure_loaded_calls.load(Relaxed),
            )
            .field(
                "ensure_loaded_wait_ns",
                &self.ensure_loaded_wait_ns.load(Relaxed),
            )
            .field(
                "ensure_loaded_work_ns",
                &self.ensure_loaded_work_ns.load(Relaxed),
            )
            .field(
                "execute_cooperative_owner_path",
                &self.execute_cooperative_owner_path.load(Relaxed),
            )
            .field(
                "execute_cooperative_joiner_path",
                &self.execute_cooperative_joiner_path.load(Relaxed),
            )
            .field(
                "execute_cooperative_held_ns",
                &self.execute_cooperative_held_ns.load(Relaxed),
            )
            .field("node_arena_pushes", &self.node_arena_pushes.load(Relaxed))
            .field(
                "node_arena_intern_miss",
                &self.node_arena_intern_miss.load(Relaxed),
            )
            .field(
                "node_arena_inner_write_wait_ns",
                &self.node_arena_inner_write_wait_ns.load(Relaxed),
            )
            .field(
                "scheduler_submit_count",
                &self.scheduler_submit_count.load(Relaxed),
            )
            .field(
                "scheduler_inbox_depth_max",
                &self.scheduler_inbox_depth_max.load(Relaxed),
            )
            .finish()
    }
}

impl MetaProvenance {
    /// Return a point-in-time snapshot of all counters.
    pub fn snapshot(&self) -> MetaProvenanceSnapshot {
        use std::sync::atomic::Ordering::Relaxed;
        MetaProvenanceSnapshot {
            get_component_meta_calls: self.get_component_meta_calls.load(Relaxed),
            component_meta_resolved_state_recomputes: self
                .component_meta_resolved_state_recomputes
                .load(Relaxed),
            get_analysis_calls: self.get_analysis_calls.load(Relaxed),
            evaluate_types_calls: self.evaluate_types_calls.load(Relaxed),
            resolved_external_type_cache_hits: self.resolved_external_type_cache_hits.load(Relaxed),
            resolved_external_type_cache_misses: self
                .resolved_external_type_cache_misses
                .load(Relaxed),
            resolver_node_cache_hits: self.resolver_node_cache_hits.load(Relaxed),
            resolver_node_cache_misses: self.resolver_node_cache_misses.load(Relaxed),
            resolver_singleflight_coalesced: self.resolver_singleflight_coalesced.load(Relaxed),
            resolver_cross_view_lane_forks: self.resolver_cross_view_lane_forks.load(Relaxed),
            resolver_cycle_detections: self.resolver_cycle_detections.load(Relaxed),
            resolver_route_fact_reuse: self.resolver_route_fact_reuse.load(Relaxed),
            resolver_barrel_fact_reuse: self.resolver_barrel_fact_reuse.load(Relaxed),
            import_resolution_cache_hit_count: self.import_resolution_cache_hit_count.load(Relaxed),
            import_resolution_cache_miss_count: self
                .import_resolution_cache_miss_count
                .load(Relaxed),
            dir_index_hit_count: self.dir_index_hit_count.load(Relaxed),
            dir_index_refresh_count: self.dir_index_refresh_count.load(Relaxed),
            dir_index_dirty_rescan_count: self.dir_index_dirty_rescan_count.load(Relaxed),
            native_fs_read_dir_count: self.native_fs_read_dir_count.load(Relaxed),
            native_fs_read_file_miss_count: self.native_fs_read_file_miss_count.load(Relaxed),
            payload_cache_hits: self.payload_cache_hits.load(Relaxed),
            payload_cache_misses: self.payload_cache_misses.load(Relaxed),
            payload_encodes: self.payload_encodes.load(Relaxed),
            indexed_ready_scheduler_snapshot_reuse: self
                .indexed_ready_scheduler_snapshot_reuse
                .load(Relaxed),
            bundle_cache_hits: self.bundle_cache_hits.load(Relaxed),
            bundle_materializations: self.bundle_materializations.load(Relaxed),
            dep_resolution_calls: self.dep_resolution_calls.load(Relaxed),
            imported_macro_declaration_builds: self.imported_macro_declaration_builds.load(Relaxed),
            route_owned_snapshot_cache_hits: self.route_owned_snapshot_cache_hits.load(Relaxed),
            route_owned_snapshot_cached_parse_hits: self
                .route_owned_snapshot_cached_parse_hits
                .load(Relaxed),
            ensure_loaded_calls: self.ensure_loaded_calls.load(Relaxed),
            ensure_loaded_wait_ns: self.ensure_loaded_wait_ns.load(Relaxed),
            ensure_loaded_work_ns: self.ensure_loaded_work_ns.load(Relaxed),
            execute_cooperative_owner_path: self.execute_cooperative_owner_path.load(Relaxed),
            execute_cooperative_joiner_path: self.execute_cooperative_joiner_path.load(Relaxed),
            execute_cooperative_held_ns: self.execute_cooperative_held_ns.load(Relaxed),
            node_arena_pushes: self.node_arena_pushes.load(Relaxed),
            node_arena_intern_miss: self.node_arena_intern_miss.load(Relaxed),
            node_arena_inner_write_wait_ns: self.node_arena_inner_write_wait_ns.load(Relaxed),
            scheduler_submit_count: self.scheduler_submit_count.load(Relaxed),
            scheduler_inbox_depth_max: self.scheduler_inbox_depth_max.load(Relaxed),
            node_arena_pushes_per_discriminant: std::array::from_fn(|i| {
                self.node_arena_pushes_per_discriminant[i].load(Relaxed)
            }),
        }
    }

    /// Reset all counters to zero.
    pub fn reset(&self) {
        use std::sync::atomic::Ordering::Relaxed;
        self.get_component_meta_calls.store(0, Relaxed);
        self.component_meta_resolved_state_recomputes
            .store(0, Relaxed);
        self.get_analysis_calls.store(0, Relaxed);
        self.evaluate_types_calls.store(0, Relaxed);
        self.resolved_external_type_cache_hits.store(0, Relaxed);
        self.resolved_external_type_cache_misses.store(0, Relaxed);
        self.resolver_node_cache_hits.store(0, Relaxed);
        self.resolver_node_cache_misses.store(0, Relaxed);
        self.resolver_singleflight_coalesced.store(0, Relaxed);
        self.resolver_cross_view_lane_forks.store(0, Relaxed);
        self.resolver_cycle_detections.store(0, Relaxed);
        self.resolver_route_fact_reuse.store(0, Relaxed);
        self.resolver_barrel_fact_reuse.store(0, Relaxed);
        self.import_resolution_cache_hit_count.store(0, Relaxed);
        self.import_resolution_cache_miss_count.store(0, Relaxed);
        self.dir_index_hit_count.store(0, Relaxed);
        self.dir_index_refresh_count.store(0, Relaxed);
        self.dir_index_dirty_rescan_count.store(0, Relaxed);
        self.native_fs_read_dir_count.store(0, Relaxed);
        self.native_fs_read_file_miss_count.store(0, Relaxed);
        self.payload_cache_hits.store(0, Relaxed);
        self.payload_cache_misses.store(0, Relaxed);
        self.payload_encodes.store(0, Relaxed);
        self.indexed_ready_scheduler_snapshot_reuse
            .store(0, Relaxed);
        self.bundle_cache_hits.store(0, Relaxed);
        self.bundle_materializations.store(0, Relaxed);
        self.dep_resolution_calls.store(0, Relaxed);
        self.imported_macro_declaration_builds.store(0, Relaxed);
        self.route_owned_snapshot_cache_hits.store(0, Relaxed);
        self.route_owned_snapshot_cached_parse_hits
            .store(0, Relaxed);
        self.ensure_loaded_calls.store(0, Relaxed);
        self.ensure_loaded_wait_ns.store(0, Relaxed);
        self.ensure_loaded_work_ns.store(0, Relaxed);
        self.execute_cooperative_owner_path.store(0, Relaxed);
        self.execute_cooperative_joiner_path.store(0, Relaxed);
        self.execute_cooperative_held_ns.store(0, Relaxed);
        self.node_arena_pushes.store(0, Relaxed);
        self.node_arena_intern_miss.store(0, Relaxed);
        self.node_arena_inner_write_wait_ns.store(0, Relaxed);
        self.scheduler_submit_count.store(0, Relaxed);
        self.scheduler_inbox_depth_max.store(0, Relaxed);
        for slot in &self.node_arena_pushes_per_discriminant {
            slot.store(0, Relaxed);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Fallthrough Resolution
// ═══════════════════════════════════════════════════════════════════════════

/// The computed fallthrough inheritance resolution for a component.
///
/// Contains the accepted surface (declared + inherited) and the
/// branch-structured inherited surface. Produced by `resolve_fallthrough_surface`.
#[derive(Debug, Clone)]
pub struct FallthroughResolution {
    /// Accepted props: declared props + inherited attrs.
    pub accepted_props: Vec<verter_semantic::analysis::component_meta::AcceptedPropAnalysis>,
    /// Accepted events: declared emits + inherited listeners.
    pub accepted_events: Vec<verter_semantic::analysis::component_meta::AcceptedEventAnalysis>,
    /// Whether the accepted surface is exact or a lower bound.
    pub accepted_surface_completeness:
        verter_semantic::analysis::component_meta::AcceptedSurfaceCompleteness,
    /// Branch-structured inherited surface.
    pub fallthrough_surface: verter_semantic::analysis::component_meta::FallthroughSurface,
    /// Semantic fact versions consumed while producing this resolution.
    pub fact_versions: Vec<crate::resolver_core::FactVersionRef>,
}

/// Serializable point-in-time snapshot of [`MetaProvenance`] counters.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetaProvenanceSnapshot {
    pub get_component_meta_calls: u64,
    pub component_meta_resolved_state_recomputes: u64,
    pub get_analysis_calls: u64,
    pub evaluate_types_calls: u64,
    pub resolved_external_type_cache_hits: u64,
    pub resolved_external_type_cache_misses: u64,
    pub resolver_node_cache_hits: u64,
    pub resolver_node_cache_misses: u64,
    pub resolver_singleflight_coalesced: u64,
    pub resolver_cross_view_lane_forks: u64,
    pub resolver_cycle_detections: u64,
    pub resolver_route_fact_reuse: u64,
    pub resolver_barrel_fact_reuse: u64,
    pub import_resolution_cache_hit_count: u64,
    pub import_resolution_cache_miss_count: u64,
    pub dir_index_hit_count: u64,
    pub dir_index_refresh_count: u64,
    pub dir_index_dirty_rescan_count: u64,
    pub native_fs_read_dir_count: u64,
    pub native_fs_read_file_miss_count: u64,
    pub payload_cache_hits: u64,
    pub payload_cache_misses: u64,
    pub payload_encodes: u64,
    pub indexed_ready_scheduler_snapshot_reuse: u64,
    pub bundle_cache_hits: u64,
    pub bundle_materializations: u64,
    pub dep_resolution_calls: u64,
    pub imported_macro_declaration_builds: u64,
    pub route_owned_snapshot_cache_hits: u64,
    pub route_owned_snapshot_cached_parse_hits: u64,
    /// Path C C1 contention instrumentation.
    pub ensure_loaded_calls: u64,
    pub ensure_loaded_wait_ns: u64,
    pub ensure_loaded_work_ns: u64,
    pub execute_cooperative_owner_path: u64,
    pub execute_cooperative_joiner_path: u64,
    pub execute_cooperative_held_ns: u64,
    pub node_arena_pushes: u64,
    pub node_arena_intern_miss: u64,
    pub node_arena_inner_write_wait_ns: u64,
    pub scheduler_submit_count: u64,
    pub scheduler_inbox_depth_max: u64,
    /// Per-`SemanticNodeData` discriminant push count.
    pub node_arena_pushes_per_discriminant: [u64; SEMANTIC_NODE_DATA_DISCRIMINANT_COUNT],
}

/// Point-in-time snapshot of host performance metrics.
///
/// Only available when the `session_metrics` feature is enabled.
/// Obtained via [`VerterHost::metrics_snapshot`](crate::VerterHost::metrics_snapshot).
#[derive(Debug, Default)]
#[cfg(feature = "session_metrics")]
pub struct HostMetricsSnapshot {
    /// Total number of `upsert()` calls.
    pub upserts: u64,
    /// Total number of compile requests (cache misses that triggered compilation).
    pub compile_requests: u64,
    /// Number of compile requests served from cache.
    pub compile_cache_hits: u64,
    /// Cache hit rate (0.0 to 1.0).
    pub compile_cache_hit_rate: f64,
    /// Total number of `get_virtual_file()` calls.
    pub virtual_loads: u64,
    /// Total number of `resolve()` calls.
    pub resolves: u64,
    /// Total number of `apply_style_overrides()` calls.
    pub style_override_calls: u64,
    /// Cumulative time spent in parse/hash phase across all upserts (microseconds).
    pub slice_hash_time_us_total: u64,
    /// Average parse/hash time per upsert (microseconds).
    pub avg_slice_hash_time_us: f64,
    /// Cumulative compilation time across all compiles (microseconds).
    pub compile_time_us_total: u64,
    /// Cumulative compilation time broken down by profile hash (microseconds).
    pub compile_time_us_total_by_profile: BTreeMap<u64, u64>,
    /// Number of compilations broken down by profile hash.
    pub compile_count_by_profile: BTreeMap<u64, u64>,
}

#[derive(Debug, Default)]
#[cfg(feature = "session_metrics")]
pub(crate) struct HostMetrics {
    pub(crate) upserts: std::sync::atomic::AtomicU64,
    pub(crate) compile_requests: std::sync::atomic::AtomicU64,
    pub(crate) compile_cache_hits: std::sync::atomic::AtomicU64,
    pub(crate) virtual_loads: std::sync::atomic::AtomicU64,
    pub(crate) resolves: std::sync::atomic::AtomicU64,
    pub(crate) style_override_calls: std::sync::atomic::AtomicU64,
    pub(crate) slice_hash_time_us_total: std::sync::atomic::AtomicU64,
    pub(crate) compile_time_us_total: std::sync::atomic::AtomicU64,
    pub(crate) compile_time_us_total_by_profile: std::sync::Mutex<HashMap<u64, u64>>,
    pub(crate) compile_count_by_profile: std::sync::Mutex<HashMap<u64, u64>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // SliceChanges tests
    // -----------------------------------------------------------------------

    /// @ai-generated — SliceChanges::is_style_only unit tests
    #[test]
    fn is_style_only_true_when_only_styles_changed() {
        let sc = SliceChanges {
            style_indices_changed: vec![0],
            ..SliceChanges::default()
        };
        assert!(sc.is_style_only());
    }

    #[test]
    fn is_style_only_true_for_multiple_style_indices() {
        let sc = SliceChanges {
            style_indices_changed: vec![0, 2],
            ..SliceChanges::default()
        };
        assert!(sc.is_style_only());
    }

    #[test]
    fn is_style_only_false_when_nothing_changed() {
        let sc = SliceChanges::default();
        assert!(!sc.is_style_only(), "no changes at all is not style-only");
    }

    #[test]
    fn is_style_only_false_when_script_also_changed() {
        let sc = SliceChanges {
            script_changed: true,
            style_indices_changed: vec![0],
            ..SliceChanges::default()
        };
        assert!(!sc.is_style_only());
    }

    #[test]
    fn is_style_only_false_when_template_also_changed() {
        let sc = SliceChanges {
            template_changed: true,
            style_indices_changed: vec![1],
            ..SliceChanges::default()
        };
        assert!(!sc.is_style_only());
    }

    #[test]
    fn is_style_only_false_when_structure_changed() {
        let sc = SliceChanges {
            structure_changed: true,
            style_indices_changed: vec![0],
            ..SliceChanges::default()
        };
        assert!(
            !sc.is_style_only(),
            "structure change means blocks were added/removed"
        );
    }

    #[test]
    fn is_style_only_false_when_descriptor_changed() {
        let sc = SliceChanges {
            descriptor_changed: true,
            style_indices_changed: vec![0],
            ..SliceChanges::default()
        };
        assert!(
            !sc.is_style_only(),
            "descriptor change (e.g. scoped added) affects compilation"
        );
    }

    #[test]
    fn is_style_only_false_for_script_only_change() {
        let sc = SliceChanges {
            script_changed: true,
            ..SliceChanges::default()
        };
        assert!(!sc.is_style_only());
    }

    #[test]
    fn is_style_only_false_for_template_only_change() {
        let sc = SliceChanges {
            template_changed: true,
            ..SliceChanges::default()
        };
        assert!(!sc.is_style_only());
    }
}
