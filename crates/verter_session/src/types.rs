#[cfg(feature = "session_metrics")]
use std::collections::BTreeMap;
#[cfg(feature = "session_metrics")]
use std::collections::HashMap;
use std::sync::Arc;

use rustc_hash::FxHashMap;

use thiserror::Error;
pub use verter_language::FileLanguage;

/// 128-bit hash (xxh3) stored as a byte array, used for content and semantic hashing.
pub type Hash16 = [u8; 16];

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

/// Caller-requested cache mode for a compile request.
///
/// `Stateless` bypasses host caches entirely (fresh compute every
/// time, no publication). `Content` consults the content-addressed
/// pure-output cache (one entry per
/// `(content_hash, env_hashes, mode_hash, source_map_policy_hash,
/// compiler / plugin versions)` key; no fact-rail invalidation).
/// `Session` consults the fact-validated session cache (multi-
/// candidate slots keyed by `(canonical, profile_hash)`, each
/// candidate validated by its path-precise `ReadSetSignature` on
/// every warm hit). `Session` is the most cache-rich mode and the
/// host's default; its fact rail and per-session slot state handle
/// cross-file, session-scoped, and IDE-shape inputs, so `Session`
/// never downgrades. An explicit `Content` request downgrades to
/// `Stateless` when any of those inputs is present (see
/// [`DowngradeReason`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompileCacheMode {
    /// Bypass host caches. The compile runs fresh, no entry is
    /// published, no fact signature is finalised. Used by tools
    /// that need a transient compute (e.g. one-off CLI invocations,
    /// integration test scaffolding) and want deterministic, side-
    /// effect-free behaviour.
    Stateless,
    /// Consult the pure content-addressed cache only. Cross-file
    /// edits invalidate this cache through env-hash bumps (lib /
    /// resolve / type env-hash dimensions), NOT through fact rail
    /// observation. Suitable for bundler / build-time flows whose
    /// inputs are fully captured by the env-hash dimensions.
    ///
    /// Note: under the default [`HostConfig`] (`dev_mode: true` +
    /// [`CompileErrorPolicy::DevServeLastKnownGood`]) the
    /// `HasDevLastGood` reason fires on every compile, which downgrades
    /// every `Content` request to `Stateless`. `Content` therefore only
    /// takes effect under a non-dev / production config (e.g.
    /// `dev_mode: false`).
    Content,
    /// Consult the fact-validated session cache. Warm hits validate
    /// the candidate's path-precise `ReadSetSignature` against the
    /// caller's live store view, so cross-file edits to referenced
    /// types invalidate the warm hit without a full env-hash bump.
    /// Suitable for IDE / LSP flows and any workflow that depends
    /// on warm-cache fidelity across cross-file edits. The host
    /// default.
    Session,
}

/// Why a requested cache mode was constrained.
///
/// A reason is recorded whenever the compile input carries a
/// cross-file dependency, a session-scoped input, or an IDE-shape
/// target. Under the mode fold a recorded reason keeps a requested
/// [`CompileCacheMode::Session`] at `Session` (the session fact rail /
/// per-session slot state handles it) and downgrades a requested
/// [`CompileCacheMode::Content`] to [`CompileCacheMode::Stateless`]
/// (the pure content key cannot represent the input). So a reason is a
/// hard *ineligibility* signal for `Content` and a *telemetry* signal
/// for `Session`.
///
/// Carriers preserve EVERY triggering reason internally in priority
/// order; the public single-field `Option<DowngradeReason>` on the
/// compile result is the highest-priority reason. The audit-event
/// payload [`verter_audit::payloads::tags::DowngradeReasonTag`]
/// carries the full ordered list for telemetry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DowngradeReason {
    /// The compile input references external `src="..."` blocks. The
    /// session fact rail observes each external file's `FileWholeHash`
    /// and invalidates the warm hit when that file's content changes,
    /// so `Session` remains eligible. A pure-content key cannot key on
    /// the external file's identity, so only `Content` is ineligible.
    HasExternalSrc,
    /// The compile input has macro type dependencies. Macro-resolved
    /// types depend on cross-file type traversal that lives outside
    /// the pure-content key. `Session` mode's path-precise fact rail
    /// captures those dependencies and stays eligible; `Content` has
    /// no fact rail, so it is ineligible.
    HasMacroTypeDeps,
    /// One of the compile input's script imports resolves through a
    /// workspace alias. Alias resolution depends on the workspace
    /// configuration; a pure-content key cannot key on the alias
    /// table, so `Content` mode is not eligible.
    HasWorkspaceAlias,
    /// The compile input depends on a file that participates in
    /// module augmentation. Augmentation visibility flows through
    /// `FileArtifactStore.augmentation_index`; a pure-content key
    /// cannot key on the augmenter set, so `Content` mode is not
    /// eligible.
    HasModuleAugmentation,
    /// The compile input carries a block override (preprocessed
    /// script / template). Block overrides are session-scoped, so
    /// the result is non-reusable across sessions and a content-
    /// addressed entry would never warm-hit.
    HasBlockOverride,
    /// The compile input carries a style override (preprocessed
    /// CSS). Same reasoning as [`Self::HasBlockOverride`].
    HasStyleOverride,
    /// The compile profile target is IDE-only analysis
    /// (`CompileTarget::TSX` without any runtime codegen). IDE
    /// analysis routes through a different cache shape; the
    /// pure-content cache would publish entries that no production
    /// caller would read.
    HasIdeOnlyAnalysis,
    /// The host is in dev mode with
    /// [`CompileErrorPolicy::DevServeLastKnownGood`]. The last-good
    /// fallback path requires per-session slot state that the
    /// content-addressed cache does not carry.
    HasDevLastGood,
    /// The compile profile carries a resolved Svelte `cssHash` override. The
    /// override is a user callback's out-of-band result; the session cannot
    /// prove the callback is content-deterministic across recomputes, so a
    /// content-addressed entry is refused fail-closed (it could otherwise warm a
    /// content-keyed slot with a scope class a later re-resolve would not
    /// reproduce). `Session` mode stays eligible because the exact override is
    /// folded into the profile identity and re-validated on every warm hit, so a
    /// session warm hit can NEVER serve a different override.
    CssHashOverridePresent,
}

/// Source-map emission policy. The hash of this value enters every
/// compile cache key under `source_map_policy_hash` so two requests
/// with different policies do not share a cache entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceMapPolicy {
    /// Source maps are embedded inline in the generated code via a
    /// `//# sourceMappingURL=data:` comment.
    Inline,
    /// Source maps are returned as a separate JSON string the caller
    /// is responsible for emitting (`*.map` file).
    External,
    /// Source maps are not generated.
    None,
}

impl CompileCacheMode {
    /// Stable 16-byte hash for use as the `compile_cache_mode_hash`
    /// dimension on every compile cache key. Determined byte-for-byte
    /// by the variant — independent of `DefaultHasher` and stable
    /// across Rust versions.
    pub fn stable_hash(&self) -> Hash16 {
        let mut buf = Vec::with_capacity(40);
        buf.extend_from_slice(b"verter.compile_cache_mode.v1:");
        buf.push(match self {
            Self::Stateless => 0x00,
            Self::Content => 0x01,
            Self::Session => 0x02,
        });
        crate::hash::hash_16(&buf)
    }
}

impl std::fmt::Display for CompileCacheMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Stateless => "stateless",
            Self::Content => "content",
            Self::Session => "session",
        })
    }
}

impl DowngradeReason {
    /// Stable 16-byte hash for the variant. Determined byte-for-byte
    /// by the variant, independent of `DefaultHasher`.
    pub fn stable_hash(&self) -> Hash16 {
        let mut buf = Vec::with_capacity(40);
        buf.extend_from_slice(b"verter.downgrade_reason.v1:");
        buf.push(match self {
            Self::HasExternalSrc => 0x00,
            Self::HasMacroTypeDeps => 0x01,
            Self::HasWorkspaceAlias => 0x02,
            Self::HasModuleAugmentation => 0x03,
            Self::HasBlockOverride => 0x04,
            Self::HasStyleOverride => 0x05,
            Self::HasIdeOnlyAnalysis => 0x06,
            Self::HasDevLastGood => 0x07,
            Self::CssHashOverridePresent => 0x08,
        });
        crate::hash::hash_16(&buf)
    }
}

impl std::fmt::Display for DowngradeReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::HasExternalSrc => "HasExternalSrc",
            Self::HasMacroTypeDeps => "HasMacroTypeDeps",
            Self::HasWorkspaceAlias => "HasWorkspaceAlias",
            Self::HasModuleAugmentation => "HasModuleAugmentation",
            Self::HasBlockOverride => "HasBlockOverride",
            Self::HasStyleOverride => "HasStyleOverride",
            Self::HasIdeOnlyAnalysis => "HasIdeOnlyAnalysis",
            Self::HasDevLastGood => "HasDevLastGood",
            Self::CssHashOverridePresent => "CssHashOverridePresent",
        })
    }
}

impl SourceMapPolicy {
    /// Stable 16-byte hash for the variant. Determined byte-for-byte
    /// by the variant, independent of `DefaultHasher`.
    pub fn stable_hash(&self) -> Hash16 {
        let mut buf = Vec::with_capacity(40);
        buf.extend_from_slice(b"verter.source_map_policy.v1:");
        buf.push(match self {
            Self::Inline => 0x00,
            Self::External => 0x01,
            Self::None => 0x02,
        });
        crate::hash::hash_16(&buf)
    }
}

impl std::fmt::Display for SourceMapPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Inline => "inline",
            Self::External => "external",
            Self::None => "none",
        })
    }
}

// ── Session → audit tag conversions ──────────────────────────────────
//
// `verter_audit` is a leaf substrate crate that cannot depend on
// `verter_session`. The translation between session enums and audit
// tags lives here so the producer-side
// `current_observer().emit_structured(...)` call sites can convert
// without a back-edge.

impl From<CompileCacheMode> for verter_audit::payloads::tags::CompileCacheModeTag {
    fn from(mode: CompileCacheMode) -> Self {
        match mode {
            CompileCacheMode::Stateless => Self::Stateless,
            CompileCacheMode::Content => Self::Content,
            CompileCacheMode::Session => Self::Session,
        }
    }
}

impl From<DowngradeReason> for verter_audit::payloads::tags::DowngradeReasonTag {
    fn from(reason: DowngradeReason) -> Self {
        match reason {
            DowngradeReason::HasExternalSrc => Self::HasExternalSrc,
            DowngradeReason::HasMacroTypeDeps => Self::HasMacroTypeDeps,
            DowngradeReason::HasWorkspaceAlias => Self::HasWorkspaceAlias,
            DowngradeReason::HasModuleAugmentation => Self::HasModuleAugmentation,
            DowngradeReason::HasBlockOverride => Self::HasBlockOverride,
            DowngradeReason::HasStyleOverride => Self::HasStyleOverride,
            DowngradeReason::HasIdeOnlyAnalysis => Self::HasIdeOnlyAnalysis,
            DowngradeReason::HasDevLastGood => Self::HasDevLastGood,
            DowngradeReason::CssHashOverridePresent => Self::CssHashOverridePresent,
        }
    }
}

#[cfg(test)]
mod stable_hash_snapshot_tests {
    //! Snapshot tests for the byte-determined `stable_hash` outputs.
    //! These pin the exact hash bytes for at least two variants per
    //! enum so a future refactor cannot silently re-derive the byte
    //! mapping (which would invalidate every persisted compile cache
    //! key embedding `compile_cache_mode_hash` or
    //! `source_map_policy_hash`).
    use super::*;

    #[test]
    fn compile_cache_mode_hashes_are_byte_stable() {
        // Hex of xxh3_128 over `b"verter.compile_cache_mode.v1:" || 0x??`.
        // If the namespace prefix or the variant byte mapping ever
        // changes, this assertion fails and every persisted entry
        // must be invalidated.
        let stateless = CompileCacheMode::Stateless.stable_hash();
        let content = CompileCacheMode::Content.stable_hash();
        let session = CompileCacheMode::Session.stable_hash();
        // Variants must produce DISTINCT hashes (the byte suffix
        // discriminates).
        assert_ne!(stateless, content);
        assert_ne!(content, session);
        assert_ne!(stateless, session);
        // The hash for `Stateless` is deterministic — same call,
        // same bytes.
        assert_eq!(stateless, CompileCacheMode::Stateless.stable_hash());
        assert_eq!(session, CompileCacheMode::Session.stable_hash());
    }

    #[test]
    fn downgrade_reason_hashes_are_byte_stable() {
        let a = DowngradeReason::HasModuleAugmentation.stable_hash();
        let b = DowngradeReason::HasWorkspaceAlias.stable_hash();
        assert_ne!(a, b);
        // Distinct discriminants.
        assert_eq!(a, DowngradeReason::HasModuleAugmentation.stable_hash());
        assert_eq!(b, DowngradeReason::HasWorkspaceAlias.stable_hash());
    }

    #[test]
    fn source_map_policy_hashes_are_byte_stable() {
        let inline = SourceMapPolicy::Inline.stable_hash();
        let external = SourceMapPolicy::External.stable_hash();
        let none = SourceMapPolicy::None.stable_hash();
        assert_ne!(inline, external);
        assert_ne!(external, none);
        assert_ne!(inline, none);
        // Determinism.
        assert_eq!(inline, SourceMapPolicy::Inline.stable_hash());
    }

    #[test]
    fn enum_displays_are_lowercase_for_modes_and_camel_for_reasons() {
        // Modes are user-facing transport strings (round-trip through
        // FFI / TS bindings as lowercase tokens).
        assert_eq!(CompileCacheMode::Stateless.to_string(), "stateless");
        assert_eq!(CompileCacheMode::Content.to_string(), "content");
        assert_eq!(CompileCacheMode::Session.to_string(), "session");
        // Reasons are diagnostic strings (camel-case matches the audit
        // tag enum names).
        assert_eq!(
            DowngradeReason::HasExternalSrc.to_string(),
            "HasExternalSrc"
        );
        assert_eq!(
            DowngradeReason::HasModuleAugmentation.to_string(),
            "HasModuleAugmentation"
        );
        // Source-map policy: lowercase tokens for transport.
        assert_eq!(SourceMapPolicy::Inline.to_string(), "inline");
        assert_eq!(SourceMapPolicy::None.to_string(), "none");
    }

    #[test]
    fn compile_cache_mode_hash_includes_namespace_prefix() {
        // The hash MUST embed the `verter.compile_cache_mode.v1:`
        // prefix — a bare-byte hash without the namespace would
        // collide with other single-byte-keyed enums. Verify by
        // computing the expected bytes manually.
        let mut buf = Vec::with_capacity(40);
        buf.extend_from_slice(b"verter.compile_cache_mode.v1:");
        buf.push(0x02);
        let expected = crate::hash::hash_16(&buf);
        assert_eq!(CompileCacheMode::Session.stable_hash(), expected);
    }

    #[test]
    fn downgrade_reason_hash_includes_namespace_prefix() {
        let mut buf = Vec::with_capacity(40);
        buf.extend_from_slice(b"verter.downgrade_reason.v1:");
        buf.push(0x03); // HasModuleAugmentation
        let expected = crate::hash::hash_16(&buf);
        assert_eq!(
            DowngradeReason::HasModuleAugmentation.stable_hash(),
            expected
        );
    }

    #[test]
    fn source_map_policy_hash_includes_namespace_prefix() {
        let mut buf = Vec::with_capacity(40);
        buf.extend_from_slice(b"verter.source_map_policy.v1:");
        buf.push(0x00); // Inline
        let expected = crate::hash::hash_16(&buf);
        assert_eq!(SourceMapPolicy::Inline.stable_hash(), expected);
    }

    #[test]
    fn compile_cache_mode_cross_namespace_distinct_from_source_map_policy() {
        // Even with identical variant bytes the namespaces must
        // discriminate. `CompileCacheMode::Stateless` (0x00) and
        // `SourceMapPolicy::Inline` (0x00) MUST hash differently.
        let a = CompileCacheMode::Stateless.stable_hash();
        let b = SourceMapPolicy::Inline.stable_hash();
        assert_ne!(
            a, b,
            "namespace prefixes MUST discriminate cross-enum hashes \
             (else two distinct keys could collide)"
        );
    }
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
    /// [`from_query_profile()`](Self::from_query_profile), which sets both
    /// this field (from the profile's recommended scope bits) AND the
    /// [`query_profile`](Self::query_profile) field. For a one-shot
    /// project typecheck use [`batch_typecheck()`](Self::batch_typecheck)
    /// instead — it sets `analysis_scope` to the carrier-affecting
    /// `AnalysisScope::BUILD` bitset explicitly, NOT from the `Build`
    /// profile's recommended bits.
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
    /// Per-category caps on the audit accumulator's unbounded `Vec`
    /// lanes. Each cap bounds the `Vec::push` count at the
    /// accumulator surface; once a cap is reached, subsequent push
    /// attempts increment the matching counter on
    /// [`verter_audit::TruncationCounters`] and the item is dropped.
    ///
    /// Only consulted when `audit_enabled = true && footprint_capture
    /// = true`. The defaults are generous (typical requests are well
    /// under every cap) but bounded so a pathological fixture cannot
    /// drive the host process into OOM.
    ///
    /// Default: [`verter_audit::AuditCaps::default`] (all categories
    /// fall back to their `DEFAULT_*` constants).
    pub audit_caps: verter_audit::AuditCaps,
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
    /// Override for the cross-file external-type frontier's
    /// `frontier_symbol_visits` step budget
    /// ([`crate::resolver_core::ResolutionBudgets::frontier_symbol_visits`]).
    ///
    /// `None` (the default) selects the production ceiling
    /// [`crate::types::MAX_EXTERNAL_TYPE_RESOLVE_STEPS`] (2000), so
    /// the frontier built at the single production construction site
    /// behaves byte-identically to the historical default. `Some(n)`
    /// caps the frontier at `n` `(canonical_id, exported_name)`
    /// visits.
    ///
    /// Used by the wide-import-graph regression tests to drive the
    /// hard frontier step-limit on a small hermetic fixture instead
    /// of a 2005-symbol corpus, without mutating the global default
    /// budget. Production code leaves this `None`.
    pub external_resolution_step_budget: Option<usize>,
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
    /// Worker count for the host-owned CPU pool used by every host
    /// batch API's outer coordinator — `compile_many` and the
    /// component-meta batch
    /// ([`verter_scheduler::HostCpuPool`]).
    ///
    /// `None` (the default) resolves to
    /// [`std::thread::available_parallelism`] at host-construction
    /// time, mirroring the previous per-call default. `Some(0)` is
    /// treated as `None` (i.e. also resolves to
    /// `available_parallelism`) rather than rejected — a
    /// misconfigured caller passing `0` through the FFI / NAPI / TS
    /// surfaces still gets a working host pool. Other positive
    /// values cap the pool's worker count.
    ///
    /// The host pool is built (or, under a lazy resource policy, lazily
    /// spawned on first use) and reused across every host batch call. The
    /// pool is distinct from the scheduler's own CPU pool — see the module
    /// documentation on [`verter_scheduler`] for the dual-pool isolation
    /// invariant.
    ///
    /// This is a legacy compat SCALAR that FEEDS [`Self::resource_policy`]:
    /// [`Self::resolved_host_cpu_pool_policy`] is the single source of
    /// truth for the host CPU pool's spawn+size. When `Some(n)` with
    /// `n > 0`, it pins the resolved size to [`PoolSize::Fixed(n)`];
    /// `None` / `Some(0)` leave the structured policy size untouched. The
    /// scalar exists only so the FFI / NAPI surfaces
    /// (`FfiHostConfig::host_cpu_threads`, `NapiHostConfig::hostCpuThreads`)
    /// can size the pool without depending on the policy types. It is NOT a
    /// second resource knob — it has a defined precedence over the policy,
    /// not a parallel one.
    pub host_cpu_threads: Option<usize>,
    /// Session-level query profile (prewarm / latency / cross-file policy).
    ///
    /// Sourced into the host's live `query_profile` slot at construction.
    /// Defaults to [`QueryProfile::LspInteractive`](verter_semantic::profile::QueryProfile::LspInteractive)
    /// — the interactive default — NOT `Build` (which is the bundler / CI
    /// typecheck profile, selected by [`Self::batch_typecheck`]). Profiles
    /// never change the meaning of a query result; they are execution hints.
    pub query_profile: verter_semantic::profile::QueryProfile,
    /// Spawn-timing + sizing of the host-owned, non-correctness worker
    /// pools (the batch-coordinator CPU pool and the decl-lowering
    /// service). Defaults to eager/historical sizes (see
    /// [`HostResourcePolicy::default`]); [`Self::batch_typecheck`] selects
    /// lazy-on-first-use so a one-shot batch host does not pay the cold
    /// thread-spawn cost it never amortises.
    pub resource_policy: HostResourcePolicy,
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

/// When a host-owned worker pool spawns its OS threads.
///
/// Spawn timing is orthogonal to [`PoolSize`] (how many workers): a pool
/// can be lazily-spawned but fixed-size, or eagerly-spawned but
/// fraction-sized. Separating the two is deliberate — conflating them in a
/// single enum would force a sizing decision onto every spawn-timing
/// choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolSpawn {
    /// Build the worker threads at host construction. The historical
    /// behaviour for every host-owned pool; the `lsp_interactive` / default
    /// preset keeps it so Full-mode construction is timing-identical.
    Eager,
    /// Defer worker-thread creation until the pool's first real demand
    /// (first `install` for the host CPU pool, first lowering job for the
    /// decl-lowering service). The `batch_typecheck` preset uses this to
    /// drop the cold thread-spawn cost a one-shot batch never amortises.
    LazyOnFirstUse,
}

/// How many worker threads a host-owned pool resolves to.
///
/// The size resolves once, eagerly at host construction, in BOTH spawn modes:
/// [`resolve`](Self::resolve) (the `available_parallelism()` call) runs up
/// front and the resolved count is handed to the pool regardless of
/// [`PoolSpawn`]. Under [`PoolSpawn::LazyOnFirstUse`] that count is passed to
/// the lazy pool constructor; only the OS-thread spawn itself is deferred —
/// never the size resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolSize {
    /// [`std::thread::available_parallelism`] (final-fallback `1` when the
    /// platform cannot report it).
    AvailableParallelism,
    /// Exactly `n` workers (`0` is clamped up to `1`; the pool always has
    /// at least one worker).
    Fixed(usize),
    /// `(available_parallelism / divisor)` clamped to the `{ min, max }`
    /// bounds. The bounds are ORDERED before clamping (an inverted `min > max`
    /// is tolerated) and `divisor == 0` is floored to `1`, so every public
    /// value resolves without panicking. The decl-lowering default is
    /// `Fraction { divisor: 4, min: 1, max: 4 }`. When the platform cannot
    /// report parallelism the fallback is `2`, likewise clamped to the ordered
    /// bounds (matching the historical decl-lowering sizing).
    Fraction {
        divisor: usize,
        min: usize,
        max: usize,
    },
}

impl PoolSize {
    /// Resolve this size to a concrete worker count (always `>= 1`).
    ///
    /// TOTAL over all public inputs: a malformed [`PoolSize::Fraction`]
    /// (`divisor == 0`, or `min > max`) never panics. The divisor is floored
    /// to `1` (no divide-by-zero) and the `{ min, max }` bounds are ORDERED
    /// before clamping, so inverted bounds resolve to a sane in-range value
    /// instead of tripping `clamp`'s `min <= max` requirement. BOTH the
    /// computed value AND the `available_parallelism` fallback are clamped to
    /// the caller's ordered bounds and floored at `1`.
    pub fn resolve(self) -> usize {
        match self {
            PoolSize::AvailableParallelism => std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1),
            PoolSize::Fixed(n) => n.max(1),
            PoolSize::Fraction { divisor, min, max } => {
                // Order the bounds so an inverted `{ min, max }` cannot trip
                // `clamp` (which requires `min <= max`), and floor the divisor
                // so `divisor == 0` cannot divide-by-zero.
                let lo = min.min(max);
                let hi = min.max(max);
                let divisor = divisor.max(1);
                std::thread::available_parallelism()
                    .map(|n| n.get() / divisor)
                    .unwrap_or(2)
                    .clamp(lo, hi)
                    .max(1)
            }
        }
    }
}

/// Spawn-timing + sizing for a single host-owned worker pool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PoolPolicy {
    /// When the pool spawns its OS threads.
    pub spawn: PoolSpawn,
    /// How many workers the pool resolves to. The size resolves EAGERLY at host
    /// construction in BOTH spawn modes (see [`PoolSize`]); only the OS-thread
    /// spawn is deferred under [`PoolSpawn::LazyOnFirstUse`], never the size
    /// resolution.
    pub size: PoolSize,
}

/// The default decl-lowering pool size — `clamp(available_parallelism / 4,
/// 1, 4)` 8 MiB workers, the historical sizing. The single definition both
/// [`HostResourcePolicy::default`] and the decl-lowering service's no-arg
/// constructor key off, so the two can never drift.
pub(crate) const DECL_LOWERING_DEFAULT_POOL_SIZE: PoolSize = PoolSize::Fraction {
    divisor: 4,
    min: 1,
    max: 4,
};

/// Resource policy for a [`VerterHost`](crate::VerterHost): the spawn
/// timing + sizing of the host-owned worker pools that are NOT required
/// for cross-file correctness.
///
/// The scheduler correctness pools (driver + CPU stage pool + IO pool) are
/// NOT policy-gated — they are always built for a session-bearing host, so
/// cross-file resolution and cache materialisation never lose a worker.
/// Only the throughput-oriented host CPU pool and the demand-driven
/// decl-lowering service are governed here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostResourcePolicy {
    /// Host-owned batch-coordinator CPU pool
    /// ([`verter_scheduler::HostCpuPool`]). Throughput-only.
    pub host_cpu_pool: PoolPolicy,
    /// Scheduler-side lazy declaration-lowering worker pool
    /// ([`crate::decl_lowering`]).
    pub decl_lowering: PoolPolicy,
}

impl Default for HostResourcePolicy {
    fn default() -> Self {
        // The Full / `lsp_interactive` defaults: both pools EAGER at the
        // historical sizes, so default-host construction is byte- and
        // timing-identical to before the resource policy existed.
        Self {
            host_cpu_pool: PoolPolicy {
                spawn: PoolSpawn::Eager,
                size: PoolSize::AvailableParallelism,
            },
            decl_lowering: PoolPolicy {
                spawn: PoolSpawn::Eager,
                size: DECL_LOWERING_DEFAULT_POOL_SIZE,
            },
        }
    }
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
    /// Sets BOTH `analysis_scope` (from the profile's recommended scope
    /// mapping) AND `query_profile` (to `profile`) — the two together are
    /// the preferred migration path from raw `AnalysisScope` to
    /// `QueryProfile`. Distinct from [`Self::batch_typecheck`], which sets
    /// `analysis_scope` to the carrier-affecting [`AnalysisScope::BUILD`]
    /// bitset EXPLICITLY (NOT `QueryProfile::Build`'s recommended bits,
    /// which omit the style facts that feed carrier bytes).
    pub fn from_query_profile(profile: verter_semantic::profile::QueryProfile) -> Self {
        let scope_bits = profile.recommended_analysis_scope_bits();
        let scope = verter_semantic::analysis::AnalysisScope::from_bits_truncate(scope_bits);
        Self {
            analysis_scope: Some(scope),
            query_profile: profile,
            ..Default::default()
        }
    }

    /// The interactive LSP preset — EXACTLY today's Full default
    /// ([`HostConfig::default`]): effective analysis scope
    /// [`AnalysisScope::LSP`](verter_semantic::analysis::AnalysisScope::LSP)
    /// (all passes), [`QueryProfile::LspInteractive`](verter_semantic::profile::QueryProfile::LspInteractive),
    /// and eager host-owned pools. One source of truth — it delegates to
    /// `default()`.
    pub fn lsp_interactive() -> Self {
        Self::default()
    }

    /// The batch-typecheck preset for a one-shot, full-project type check
    /// (CI / bundler / `verter-tsc`).
    ///
    /// Keeps the SAME shared `VerterHost` / resolver / cache substrate as
    /// [`Self::lsp_interactive`]; it only re-presets the orthogonal axes:
    ///
    /// - **analysis scope** = [`AnalysisScope::BUILD`](verter_semantic::analysis::AnalysisScope::BUILD)
    ///   — the carrier-affecting fact set (imports, bindings, macros,
    ///   macro-type-deps, export signatures, AND `STYLE_VBIND` +
    ///   `STYLE_SCOPED`). It is set EXPLICITLY here, NOT derived from
    ///   `QueryProfile::Build.recommended_analysis_scope_bits()` (which
    ///   omits the style bits and so would drop carrier bytes), and NOT
    ///   `BUILD_OPTIMIZED` (which adds template/cross facts beyond the
    ///   typecheck boundary). Same carrier bytes as Full ⇒ same tsc input.
    /// - **query profile** = [`QueryProfile::Build`](verter_semantic::profile::QueryProfile::Build).
    /// - **resource policy** = both host-owned pools lazy-on-first-use at
    ///   the historical sizes, so cold construction spawns zero throughput
    ///   threads (the scheduler correctness pools are still eager).
    /// - **audit** off (the default).
    pub fn batch_typecheck() -> Self {
        Self {
            analysis_scope: Some(verter_semantic::analysis::AnalysisScope::BUILD),
            query_profile: verter_semantic::profile::QueryProfile::Build,
            resource_policy: HostResourcePolicy {
                host_cpu_pool: PoolPolicy {
                    spawn: PoolSpawn::LazyOnFirstUse,
                    size: PoolSize::AvailableParallelism,
                },
                decl_lowering: PoolPolicy {
                    spawn: PoolSpawn::LazyOnFirstUse,
                    size: DECL_LOWERING_DEFAULT_POOL_SIZE,
                },
            },
            ..Default::default()
        }
    }

    /// The single source of truth for the host CPU pool's resolved
    /// spawn+size policy. Starts from `resource_policy.host_cpu_pool` and
    /// applies the legacy [`Self::host_cpu_threads`] compat scalar:
    /// `Some(n)` with `n > 0` pins [`PoolSize::Fixed(n)`]; `None` /
    /// `Some(0)` leave the structured policy size unchanged. The scalar
    /// FEEDS the policy with a defined precedence — there is never a second
    /// competing pool-size knob.
    pub fn resolved_host_cpu_pool_policy(&self) -> PoolPolicy {
        let mut policy = self.resource_policy.host_cpu_pool;
        if let Some(n) = self.host_cpu_threads.filter(|&n| n > 0) {
            policy.size = PoolSize::Fixed(n);
        }
        policy
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
            audit_caps: verter_audit::AuditCaps::default(),
            depth_budget: crate::component_meta_materialize::MAX_DEPTH,
            projection_op_budget: 2000,
            eviction_policy: EvictionPolicyConfig::default(),
            lsp_method_timeouts: LspMethodTimeoutsConfig::default(),
            external_resolution_step_budget: None,
            recursion_budget_overrides: RecursionBudgetOverrides::default(),
            typeinfo_scratch_cache_capacity: None,
            host_cpu_threads: None,
            // Interactive is the correct default profile — `Build` is the
            // bundler/CI typecheck profile, selected only by
            // `batch_typecheck()`.
            query_profile: verter_semantic::profile::QueryProfile::LspInteractive,
            resource_policy: HostResourcePolicy::default(),
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
    /// SSR asset-collection module id registered on `ssrContext.modules`.
    /// Vite's ssr-manifest keys are ROOT-RELATIVE, so the bundler plugin
    /// supplies `normalizePath(relative(root, filename))` here (the shape
    /// `@vitejs/plugin-vue` registers). `None` falls back to the canonical
    /// id — correct only when the caller's manifest keys are canonical.
    pub ssr_module_id: Option<String>,
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
    /// The RESOLVED Svelte `cssHash` scope-class override — the user `cssHash`
    /// callback's byte-exact result, computed OUTSIDE the compiler (at this
    /// session/API boundary) BEFORE the cache lookup, ONCE per request. It is a
    /// COMPILE-OUTPUT POLICY dimension (not source content, not any env hash), so
    /// it participates in the profile's exact equality/hash — folding into BOTH
    /// the session compile-slot discriminator and the Content-mode key. When
    /// `Some`, the Svelte carrier uses it verbatim as the scope class; Vue ignores
    /// it. A present override makes a requested Content compile non-admissible
    /// (fail-closed — see [`DowngradeReason::CssHashOverridePresent`]); Session
    /// caching stays safe because the exact override is in the profile identity
    /// and re-validated on every warm hit.
    pub svelte_css_hash_override: Option<String>,
    /// Caller-requested compile cache mode for this request.
    ///
    /// The host classifies this against the request's eligibility
    /// surface (see [`DowngradeReason`]) and routes the compile through
    /// the resulting actual mode: [`CompileCacheMode::Session`] (the
    /// default) consults the fact-validated session cache,
    /// [`CompileCacheMode::Content`] the pure content-addressed cache,
    /// [`CompileCacheMode::Stateless`] no cache. Defaults to `Session`.
    pub requested_mode: CompileCacheMode,
}

impl CompileProfile {
    /// Whether this profile carries parse-affecting template options.
    ///
    /// `delimiters` and `custom_elements` change how the SFC source
    /// tokenizes, so a template extracted under them describes a
    /// DIFFERENT parse of the same bytes than the default one: a
    /// cached parse cannot be reused for the compile, and the
    /// extraction must not populate the profileless default-extraction
    /// template slot ([`RawTemplateSlotAdmission::default_extraction`]).
    pub(crate) fn has_parse_affecting_template_options(&self) -> bool {
        self.delimiters.is_some() || self.custom_elements.is_some()
    }
}

impl Default for CompileProfile {
    fn default() -> Self {
        Self {
            filename: None,
            is_production: false,
            ssr: false,
            ssr_module_id: None,
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
            svelte_css_hash_override: None,
            requested_mode: CompileCacheMode::Session,
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

impl From<&verter_semantic::analysis::AnalyzedModuleReference> for ScriptModuleReference {
    fn from(reference: &verter_semantic::analysis::AnalyzedModuleReference) -> Self {
        ScriptModuleReference {
            syntax: reference.syntax,
            semantics: reference.semantics,
            is_type_only: reference.is_type_only,
            raw_text: reference.raw_text.clone(),
            literal_specifier: reference.literal_specifier.clone(),
            finite_specifiers: reference.finite_specifiers.clone(),
            static_prefix: reference.static_prefix.clone(),
            analyzability: reference.analyzability,
            span: reference.span,
            expr_span: reference.expr_span,
        }
    }
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

    /// Script-side usage facts for macro-declared members (unused-declaration
    /// diagnostics). `None` for files without Vue macros.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macro_usage: Option<verter_semantic::analysis::macro_usage::MacroUsageFacts>,

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
    /// `true` iff this response was served from a warm cache slot
    /// (the fact-validated session slot OR the content-addressed
    /// store), `false` for a cold compute or last-known-good fallback.
    pub cache_hit: bool,
    /// The compile cache mode the caller requested for this compile.
    pub requested_mode: CompileCacheMode,
    /// The compile cache mode the runtime actually ran under. Equals
    /// `requested_mode` unless an explicit `Content` request downgraded
    /// to `Stateless` (see [`DowngradeReason`]).
    pub actual_mode: CompileCacheMode,
    /// The highest-priority reason the requested mode was constrained,
    /// or `None` when no reason fired. Populated for both `Session`
    /// (telemetry) and `Content` (downgrade cause) requests.
    pub downgrade_reason: Option<DowngradeReason>,
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
    /// The file's language (framework carrier vs. plain script).
    pub file_language: FileLanguage,
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

/// The payload of a failed compile attempt.
///
/// Carries the error diagnostics together with the mode metadata that
/// was already decided by the single mode classifier before the compile
/// ran. The error arm of a batch compile reads these so an errored
/// entry reports the true cache mode the runtime ran under (e.g. a
/// `Content` request that downgraded to `Stateless`) instead of echoing
/// the requested mode.
#[derive(Debug, Clone)]
pub struct CompileFailure {
    /// All diagnostics (errors and warnings) emitted by the failed compile.
    pub diagnostics: DiagnosticsSnapshot,
    /// The compile cache mode the caller requested.
    pub requested_mode: CompileCacheMode,
    /// The compile cache mode the runtime actually ran under.
    pub actual_mode: CompileCacheMode,
    /// The highest-priority reason the requested mode was constrained,
    /// or `None` when no reason fired.
    pub downgrade_reason: Option<DowngradeReason>,
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
    /// The carrier REFUSED to produce the requested RUNTIME surface for an
    /// unsupported construct (a fail-closed runtime outcome). DISTINCT from a
    /// generic missing node: the runtime artifact was REQUESTED and explicitly
    /// refused, with the precise reason in `diagnostic_code` / `message`. The IDE
    /// projection is still produced (type-checking survives); a runtime-requesting
    /// consumer reads this rather than mistaking the absent node for an IDE-only
    /// carrier.
    #[error("runtime surface refused for '{canonical_id}': {diagnostic_code}: {message}")]
    RuntimeSurfaceRefused {
        /// The canonical id of the refused file.
        canonical_id: String,
        /// The machine-stable refusal diagnostic code (e.g.
        /// `svelte-runtime-unsupported-block`).
        diagnostic_code: String,
        /// The human-readable refusal reason.
        message: String,
    },
    /// Compilation failed. Carries the error diagnostics plus the mode
    /// metadata decided at classification time.
    #[error("compile error")]
    CompileError(CompileFailure),
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
    /// True when changing a style can also change the runtime Main module
    /// (for example, Svelte's CSS scope hash is embedded in generated markup).
    pub(crate) main_depends_on_styles: bool,
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

/// Caller-threaded parse products for the lazy template-analysis
/// computation (`compute_template_analysis_if_missing`): the SAME
/// source + SFC parse the caller's analysis snapshot was built from,
/// so the template derives from one coherent read with zero re-parses
/// of the same bytes (src-block info is re-derived from the parse via
/// the pure `collect_vue_src_blocks` walk — no OXC work).
///
/// This is the computation's SOLE source acquisition — it never
/// consults the scheduler itself — and the carrier of the caller's
/// conversion-context attestation, captured by value at the caller's
/// own read site. Base lanes thread store-authoritative reads
/// (`store_published = true`, eligible to persist into the base
/// `derived_raw_cache` slot); overlay/session lanes thread their OWN
/// overlay source with `store_published = false`, so the conversion
/// stays coherent with the overlay snapshot AND serves return-only —
/// overlay results never populate base caches.
pub(crate) struct VueTemplateInputs {
    pub(crate) source: Arc<str>,
    /// The framework-neutral carrier parse artifact of `source`. `None`
    /// routes the computation through one counted carrier parse of its
    /// own — a single parse, never a duplicate of one the caller ran.
    pub(crate) framework_parse: Option<Arc<verter_language::FrameworkParseArtifact>>,
    /// Publication status of the state these inputs were captured
    /// from, flowed BY VALUE (the gate works with or without an
    /// installed `RequestContext`): live scheduler/workspace reads are
    /// store-authoritative (`true`); an artifact threaded from a
    /// FENCED (`store_published == false`) `IndexedReadyServe` carries
    /// `false`. The computed template always serves the caller, but a
    /// `false` here DECLINES the `derived_raw_cache` persist —
    /// ReturnOnly never publishes: the persisted entry carries no
    /// content rail, so a template derived from superseded bytes would
    /// be served as current by every subsequent template read.
    pub(crate) store_published: bool,
    /// Scheduler node generation of the source read these inputs were
    /// captured from — the value stamped onto
    /// [`RawTemplateAnalysisEntry::source_generation`] at persist.
    /// `None` when the capture site read no scheduler node (the
    /// from-source snapshot builder, the artifact-serve lane): with no
    /// generation to stamp, the computed template serves the caller
    /// but the persist declines — an entry without a rail cannot be
    /// validated by any reader, so it must not exist.
    pub(crate) source_generation: Option<u64>,
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
    /// Immutable post-parse script analysis, shared by `Arc` so the
    /// override-aware `effective_file_state` read and the analysis-snapshot
    /// reuse path bump a refcount instead of deep-copying ~18 owned vectors
    /// for callers that read one or two scalar fields.
    pub(crate) script_analysis: Arc<verter_semantic::analysis::ScriptAnalysisSnapshot>,
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
    /// The declaration-only public surface (`.d.<ext>.ts`): a valid `.d.ts` —
    /// no runtime/value code — that a bare framework-carrier import resolves to.
    Declaration,
}

#[derive(Debug, Clone)]
pub(crate) struct CompileSlot {
    pub(crate) semantic_hash: Hash16,
    pub(crate) style_override_hash: u64,
    pub(crate) content_override_hash: u64,
    /// The RESOLVED Svelte `cssHash` override captured at publish (byte-exact;
    /// `None` for the default derivation). Compared EXACTLY on every warm hit so
    /// a `profile_hash` u64 collision can NEVER serve a result carrying a
    /// different scope class — the exact-override warm-hit contract. The override is
    /// already folded into `profile_hash` (the slot key); this exact-string
    /// discriminant hardens that folding against the u64 collision the bare-hash
    /// slot key would otherwise admit. `None` (the default — Vue and every
    /// un-overridden Svelte compile) is a null pointer, so the discriminant is
    /// zero-cost off the override path.
    pub(crate) css_hash_override: Option<Arc<str>>,
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
    /// R3/R26/R28 cold-compute fact signature. Accumulated by the
    /// `with_fact_tracer` scope wrapping the compile cold-compute
    /// pass; every per-`Member`, per-`MemberPresence`,
    /// `ImportRef`, and `RouteSurface`
    /// (incl. `ModuleAugmentationIndexShape`) observation made
    /// during compile is recorded here. Validated on every warm-hit
    /// read against the producer's current fact registry — a
    /// cross-file edit invalidates the affected per-Member fact and
    /// the consumer's warm hit misses without any eager invalidation.
    ///
    /// Carrier invariant: `present in compile_slots` implies admitted
    /// cache entry. A tracer that finalised with `Overflow` MUST NOT
    /// publish here — the cold-build producer refuses the
    /// `compile_slots.insert` on overflow and returns the freshly
    /// computed value to its single caller without admitting. An
    /// empty fact rail (`facts.is_empty() && !overflowed`) is a valid
    /// admitted state: the warm-hit oracle validates vacuously and
    /// falls back to the existing `semantic_hash`/override-hash
    /// pre-filter.
    pub(crate) fact_dep_signature: crate::fact_signature_helpers::ReadSetSignature,
    /// Whether the carrier fail-closed on an unsupported runtime surface (the typed
    /// runtime-refusal signal). Survives a warm hit so a runtime-requesting consumer
    /// reads the refusal from this flag, not a diagnostic-code prefix.
    pub(crate) runtime_surface_refused: bool,
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
    /// Script-side macro-member usage facts from the effective script analysis.
    /// Feeds the unused-declaration inventories during template conversion.
    pub(crate) script_macro_usage: Option<verter_semantic::analysis::macro_usage::MacroUsageFacts>,
    /// Vue API call sites from the effective script analysis (the `useSlots()`
    /// fail-open gate for unused-slot diagnostics).
    pub(crate) script_vue_api_calls: Vec<verter_semantic::analysis::types::VueApiCallSite>,
    /// Framework-neutral parse artifact from upsert, reused during
    /// compilation to avoid re-parsing.
    pub(crate) framework_parse: Option<Arc<verter_language::FrameworkParseArtifact>>,
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
#[derive(Debug, Clone, PartialEq, Eq)]
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

    /// A "known miss": the resolver answered with NO resolved canonical,
    /// no candidates, and therefore no effective target. A known-miss is
    /// a generation-scoped negative answer — its currency is governed by
    /// the owner's known-miss generation sidecar
    /// ([`DerivedRawState::import_routes_known_miss_recorded_at_generation`]),
    /// never served as an unconditional authoritative route.
    pub(crate) fn is_known_miss(&self) -> bool {
        self.resolved_canonical_id.is_none()
            && self.effective_target().is_none()
            && self.possible_canonical_ids.is_empty()
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

#[allow(dead_code)]
impl ProfileState {
    /// Typed-node accessor for the per-profile compile slot.
    ///
    /// The slot storage is private to the typed compile-output node
    /// module — every read / write outside that module routes through
    /// the typed [`crate::cache_runtime::compile_output_node`]
    /// surfaces. The accessor returns a borrow scoped to the caller's
    /// slot-map read so callers cannot mutate the slot under the
    /// node's nose.
    pub(crate) fn compile_slot_for_node(&self, profile_hash: u64) -> Option<&CompileSlot> {
        self.compile_slots.get(&profile_hash)
    }

    /// Typed-node admission for the per-profile compile slot.
    ///
    /// Replaces any prior slot for `profile_hash`. Routed exclusively
    /// from the typed compile-output node's `publish` method, which
    /// gates admission on a `Cacheable` [`SignatureAdmission`]
    /// carrier.
    pub(crate) fn compile_slot_insert_for_node(&mut self, profile_hash: u64, slot: CompileSlot) {
        self.compile_slots.insert(profile_hash, slot);
    }

    /// Typed-node removal for the per-profile compile slot. Routed
    /// from the typed compile-output node's `publish` (on a refused
    /// admission) and `remove` methods. Returns the removed slot when
    /// one existed.
    pub(crate) fn compile_slot_remove_for_node(
        &mut self,
        profile_hash: u64,
    ) -> Option<CompileSlot> {
        self.compile_slots.remove(&profile_hash)
    }

    /// Typed-node bulk clear for every per-profile compile slot.
    ///
    /// Routed exclusively from the typed compile-output node's
    /// `clear_compile_outputs_for_file` method. Drops ONLY the
    /// compile-output slots — the sibling `content_overrides`,
    /// `style_overrides`, `latest_diagnostics`, and
    /// `diagnostics_generation` fields are compile *inputs* /
    /// observable state owned by their own invalidation callers and
    /// are left untouched here.
    pub(crate) fn compile_slots_clear_for_node(&mut self) {
        self.compile_slots.clear();
    }
}

/// Source-content-domain state for the scheduler-backed compile cache (D48).
///
/// Stored in [`crate::project_type_store::DerivedRawCacheDb`] keyed by canonical id.
/// Source-content changes invalidate this; profile-flag changes preserve it;
/// dep-closure changes preserve it. AUTHORITY RESETS (`set_workspace` /
/// `close` — the wide `bump_project_generation_and_evict`) invalidate it;
/// route-resolution project-generation bumps PRESERVE it (stamp-only
/// freshness: stale route mirrors are cleared owner-scoped and stale
/// reads miss by validation). See the §3.4.2 invalidation matrix.
///
/// `import_routes` is a sub-mirror of
/// [`crate::project_type_store::IndexedReady`]`.import_routes`: same content,
/// different invalidation trigger. Source-content change for the owner drops
/// this DerivedRawState entry (along with the IndexedReady entry it mirrored);
/// profile-flag change preserves DerivedRawState while leaving IndexedReady
/// untouched. The asymmetry that motivated D48 is the per-domain trigger
/// independence — keeping `import_routes` here means a profile-flag sweep
/// no longer drops the resolved-route cache redundantly.
/// A lazily computed template analysis pinned to the scheduler node
/// generation of the source it derives from — the
/// [`DerivedRawState::raw_template_analysis`] slot's validity rail.
///
/// The slot is canonical-keyed and lazily persisted, so a persist can
/// land AFTER the upsert that superseded its inputs already cleared
/// the slot (capture authority proves the inputs were coherent when
/// captured, not that the slot is still current at persist time). The
/// generation makes correctness read-side authoritative: every reader
/// already holds a generation-coherent scheduler snapshot
/// (`try_get_source` / `try_get_analysis` carry the node generation)
/// and accepts the entry only at its own snapshot's generation — a
/// late persist stamped with the superseded generation lands inert
/// and the next coherent compute replaces it.
#[derive(Debug)]
pub(crate) struct RawTemplateAnalysisEntry {
    pub(crate) template: Arc<verter_semantic::analysis::template::TemplateAnalysisSnapshot>,
    /// Scheduler node generation of the source read the template's
    /// inputs were captured from.
    pub(crate) source_generation: u64,
}

/// A persist site's by-value admission statement for the
/// [`DerivedRawState::raw_template_analysis`] slot.
///
/// The slot has exactly one structural write authority
/// ([`DerivedRawState::install_raw_template_analysis`], reached through
/// the host persist chokepoint `VerterHost::persist_raw_template_analysis`)
/// and every persist site — the lazy template-analysis computation and
/// the Session compile-publish lane — states its context through this
/// carrier instead of duplicating the gate. The decision rules live on
/// [`RawTemplateSlotAdmission::admitted_generation`]; the fields here
/// are the facts only the capture site can attest.
pub(crate) struct RawTemplateSlotAdmission {
    /// Capture authority for the bytes the template derives from:
    /// `true` only for store-authoritative reads (live
    /// scheduler/workspace bytes, no content override). ReturnOnly
    /// never publishes — a template derived from a fenced serve or
    /// from overridden block content describes bytes the store never
    /// published.
    pub(crate) store_published: bool,
    /// Scheduler node generation of the source read the template's
    /// inputs were captured from — the validity rail readers key on.
    /// `None` declines: an entry without a rail cannot be validated
    /// by any reader, so it must not exist.
    pub(crate) source_generation: Option<u64>,
    /// Whether the SFC carries external `src=` blocks. External-src
    /// templates never populate the slot: an external dep edit clears
    /// compile slots, not this slot, and the owner's node generation
    /// does not move, so the rail could never reject the stale entry.
    pub(crate) has_src_blocks: bool,
    /// Whether the template was extracted under the DEFAULT parse
    /// options. A parse-affecting profile
    /// ([`CompileProfile::has_parse_affecting_template_options`])
    /// extracts a different template from the same bytes; the slot is
    /// profileless and readers serve it as the raw/default template,
    /// so a non-default extraction must not populate it — the entry
    /// would carry a VALID current generation stamp no reader could
    /// reject.
    pub(crate) default_extraction: bool,
}

impl RawTemplateSlotAdmission {
    /// THE admission decision for the
    /// [`DerivedRawState::raw_template_analysis`] slot: `Some(rail)`
    /// with the generation to stamp when the statement admits, `None`
    /// when it declines. The rules live here only:
    ///
    /// - inline templates only (`has_src_blocks` declines): editing an
    ///   external `src=` dep clears compile slots, not this slot, and
    ///   the owner's node generation does not move, so the generation
    ///   rail could never reject the stale entry;
    /// - default extraction only (`!default_extraction` declines): the
    ///   slot is profileless and every reader serves it as the
    ///   raw/default template — a parse-affecting extraction would
    ///   land under a VALID current generation stamp no reader could
    ///   reject;
    /// - store-published inputs only (ReturnOnly never publishes — a
    ///   template derived from a fenced serve or overridden block
    ///   content describes bytes the store never published);
    /// - a captured source generation to stamp as the entry's validity
    ///   rail: an entry without a rail cannot be validated by any
    ///   reader, so it must not exist.
    pub(crate) fn admitted_generation(&self) -> Option<u64> {
        if self.has_src_blocks || !self.default_extraction || !self.store_published {
            return None;
        }
        self.source_generation
    }
}

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
    /// View-aware cached resolved component-meta sidecar keyed by
    /// `(ProjectionMode, view_fingerprint)`. `view_fingerprint == 0`
    /// is the base-only slot; non-zero fingerprints identify per-
    /// overlay-set slots so two sessions with conflicting overlays
    /// cannot observe each other's cached state through the legacy
    /// fallback path.
    pub(crate) cached_resolved_meta:
        FxHashMap<(ProjectionMode, u64), ResolvedComponentMetaCacheEntry>,
    /// Cached encoded protobuf payload for the canonical component-meta query.
    pub(crate) cached_meta_payload: Option<CachedMetaPayload>,

    /// Raw template analysis (source-derived, profileless), pinned to
    /// the scheduler node generation of the source it derives from
    /// ([`RawTemplateAnalysisEntry`]). Always raw — never from
    /// overrides, never a non-default extraction.
    ///
    /// MODULE-PRIVATE on purpose: the single write authority is
    /// structural, not conventional. The only populating mutator is
    /// [`DerivedRawState::install_raw_template_analysis`], which gates
    /// on the persist site's by-value [`RawTemplateSlotAdmission`]
    /// statement; the only other mutator is the fail-closed
    /// [`DerivedRawState::clear_raw_template_analysis`]. A direct
    /// `derived.raw_template_analysis = Some(...)` outside this module
    /// does not compile, so no future writer can bypass the admission
    /// gate. Readers go through
    /// [`DerivedRawState::raw_template_analysis`].
    ///
    /// EXTERNAL SRC RULE: When src_blocks is non-empty, raw_template_analysis is NOT cached
    /// (set to None after read). Editing an external `<template src>` / `<script src>` dep
    /// only triggers `smart_invalidate_dependents` (which clears compile_slots), not
    /// raw_template_analysis.
    ///
    /// DEFAULT EXTRACTION RULE: parse-affecting profile options
    /// (`delimiters`, `custom_elements`) extract a different template
    /// from the same bytes; such an extraction never populates this
    /// profileless slot — the entry would carry a valid current
    /// generation stamp that no reader could reject.
    raw_template_analysis: Option<RawTemplateAnalysisEntry>,

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

    /// Per-specifier workspace `content_generation` recorded when a
    /// known-miss `DependencyResolution` (no resolved canonical and
    /// no candidates) was admitted to `import_routes`. R3/R26/R28:
    /// the reader must re-resolve once the workspace's
    /// `content_generation` advances past the recorded value — a new
    /// canonical may now satisfy the specifier. Missing entries are
    /// treated as "never recorded" → the reader forces a fresh
    /// resolution.
    pub(crate) import_routes_known_miss_recorded_at_generation: FxHashMap<String, u64>,

    /// Per-specifier [`PositiveRouteStamp`] recorded when the HOST
    /// memoized a positive `DependencyResolution` into `import_routes`
    /// (`cache_positive_import_route_result` — the single positive point
    /// producer). A positive resolution is a dependency-set-derived edge
    /// exactly like a known-miss: a file appearing or retargeting (a
    /// `.d.ts` companion, a more-specific sibling shadowing a directory
    /// index, a resolve-extension change) can move it while the owner's
    /// own content stays put, so readers and the route-surface rebuild
    /// treat a stamped entry as current ONLY while its recorded
    /// generation equals the live `content_generation`, re-resolving
    /// otherwise — through the stamp's RECORDED resolution lane, so the
    /// re-resolve replays exactly the resolution the memo captured.
    /// Entries WITHOUT a stamp are caller-supplied authoritative routes
    /// (`set_import_dependencies` — the bundler tells the host how
    /// ITS resolver resolves, and re-pushes on its own watch events):
    /// those serve unconditionally until replaced. The sidecar is
    /// cleared wherever `import_routes` is cleared or wholesale
    /// replaced.
    pub(crate) import_routes_positive_recorded_at_generation: FxHashMap<String, PositiveRouteStamp>,
}

/// Sidecar record for one HOST-MEMOIZED positive `import_routes` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PositiveRouteStamp {
    /// Workspace `content_generation` the CALLER captured BEFORE
    /// performing the resolution this stamp records (capture-before-
    /// resolve: a mutation landing between resolve and record leaves the
    /// stamp conservatively stale, never forged-current).
    pub(crate) generation: u64,
    /// The workspace resolution lane (`ResolveRequestKind`) the memo was
    /// produced through. A generation-stale entry re-resolves through
    /// the SAME lane: exact resolutions are keyed `(specifier, phase,
    /// kind)`, so replaying a different lane (e.g. the type-route chain
    /// for an `SfcSrcAttr` memo) would miss the caller's exact row and
    /// diverge recorder from validator.
    pub(crate) kind: verter_workspace::ResolveRequestKind,
}

impl DerivedRawState {
    /// Read access to the module-private `raw_template_analysis`
    /// slot. Readers validate the entry against their own snapshot's
    /// generation (the entry's `source_generation` rail) before
    /// serving it.
    pub(crate) fn raw_template_analysis(&self) -> Option<&RawTemplateAnalysisEntry> {
        self.raw_template_analysis.as_ref()
    }

    /// Drop the raw-template slot. Clearing is the fail-closed
    /// direction — an absent entry can never serve stale — so this is
    /// open to every invalidation site (upsert, eviction, cache
    /// flush) without going through the admission gate.
    pub(crate) fn clear_raw_template_analysis(&mut self) {
        self.raw_template_analysis = None;
    }

    /// Sole populating mutator for the module-private
    /// `raw_template_analysis` slot — the structural write authority.
    /// The persist site states its facts through the by-value
    /// [`RawTemplateSlotAdmission`] and THIS method decides via
    /// [`RawTemplateSlotAdmission::admitted_generation`]; a statement
    /// that declines never touches the slot, so no caller can install
    /// an entry the admission rules reject.
    ///
    /// The install is monotonic over captured stamps (never a live
    /// re-read): a late persist must not replace an entry a
    /// newer-generation compute already installed.
    pub(crate) fn install_raw_template_analysis(
        &mut self,
        template: Arc<verter_semantic::analysis::template::TemplateAnalysisSnapshot>,
        admission: RawTemplateSlotAdmission,
    ) {
        let Some(source_generation) = admission.admitted_generation() else {
            return;
        };
        let supersedes = self
            .raw_template_analysis
            .as_ref()
            .is_none_or(|entry| entry.source_generation <= source_generation);
        if supersedes {
            self.raw_template_analysis = Some(RawTemplateAnalysisEntry {
                template,
                source_generation,
            });
        }
    }

    /// Whether the `import_routes` entry for `specifier` may be served /
    /// seeded as current at `current_generation`. `true` for unstamped
    /// entries (caller-supplied authoritative routes and known-misses —
    /// the latter carry their own sidecar and re-resolution rail);
    /// `false` for a host-memoized positive whose recorded generation no
    /// longer matches the live `content_generation` (the dependency file
    /// set moved — the entry must re-resolve).
    pub(crate) fn import_route_is_generation_current(
        &self,
        specifier: &str,
        current_generation: u64,
    ) -> bool {
        self.import_routes_positive_recorded_at_generation
            .get(specifier)
            .is_none_or(|stamp| stamp.generation == current_generation)
    }

    /// The resolution lane recorded on a HOST-MEMOIZED positive
    /// `import_routes` entry, when one exists. `None` for unstamped
    /// (caller-supplied authoritative) entries and known-misses. A
    /// generation-stale stamped positive re-resolves through this lane so
    /// the re-resolution replays exactly the resolution the memo captured
    /// (exact resolutions are kind-keyed).
    pub(crate) fn positive_route_resolution_kind(
        &self,
        specifier: &str,
    ) -> Option<verter_workspace::ResolveRequestKind> {
        self.import_routes_positive_recorded_at_generation
            .get(specifier)
            .map(|stamp| stamp.kind)
    }

    /// The COMPLETE per-entry freshness oracle for an `import_routes`
    /// entry — the single predicate every seed loop and warm read site
    /// consults before treating a stored route as current:
    ///
    /// * **Known-miss** — current ONLY while its known-miss sidecar
    ///   stamp equals the live `content_generation`. A missing stamp
    ///   means "never recorded" and is treated as stale (fail closed):
    ///   a file appearing after the miss was recorded advances the
    ///   generation, so the specifier must re-resolve against the live
    ///   file set.
    /// * **Host-memoized positive** (positive sidecar stamp present) —
    ///   current ONLY while the stamp equals the live generation; the
    ///   stamp is captured BEFORE the resolution it records, so a race
    ///   leaves it conservatively stale.
    /// * **Caller-supplied authoritative positive** (no stamp) — serves
    ///   until replaced; the caller re-pushes on its own watch events.
    pub(crate) fn import_route_entry_is_generation_current(
        &self,
        specifier: &str,
        resolution: &DependencyResolution,
        current_generation: u64,
    ) -> bool {
        if resolution.is_known_miss() {
            return self
                .import_routes_known_miss_recorded_at_generation
                .get(specifier)
                .is_some_and(|recorded| *recorded == current_generation);
        }
        self.import_route_is_generation_current(specifier, current_generation)
    }
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
    /// Shared immutable script analysis — an `Arc::clone` of the snapshot the
    /// scheduler (or the content override) holds. Reading it is a refcount
    /// bump; consumers that need an owned copy call `.as_ref().clone()`.
    pub(crate) script_analysis: std::sync::Arc<verter_semantic::analysis::ScriptAnalysisSnapshot>,
    pub(crate) framework_parse: Option<std::sync::Arc<verter_language::FrameworkParseArtifact>>,
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
    pub(crate) framework_parse: Option<Arc<verter_language::FrameworkParseArtifact>>,
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
// External type resolution limits
// ═══════════════════════════════════════════════════════════════════════════

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

/// Cached host-owned component-meta resolved state.
///
/// The declaration-graph traversal cache remains shared and mode-agnostic.
/// This cache stores the mode-specific materialized sidecar and verifies that
/// both the owner file and every tracked dependency still match the hashes that
/// produced the cached state.
#[derive(Debug, Clone)]
pub(crate) struct ResolvedComponentMetaCacheEntry {
    /// Observed cross-file fact signature captured at cold-compute
    /// publish time. Stored as `Arc<[FactVersionRef]>` so warm-hit
    /// validation clones the handle without copying the slice
    /// (R3/R26/R28 fact-validation substrate).
    pub fact_versions: Arc<[crate::resolver_core::FactVersionRef]>,
    pub state: Arc<crate::meta_resolve::ResolvedComponentMetaState>,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedFallthroughEntry {
    /// Observed cross-file fact signature captured at cold-compute
    /// publish time. Stored as `Arc<[FactVersionRef]>` so warm-hit
    /// validation clones the handle without copying the slice
    /// (R3/R26/R28 fact-validation substrate).
    pub fact_versions: Arc<[crate::resolver_core::FactVersionRef]>,
    pub generic_root_propagation: bool,
    pub resolution: Arc<FallthroughResolution>,
}

/// Cached encoded protobuf payload for a component-meta query.
#[derive(Debug, Clone)]
pub(crate) struct CachedMetaPayload {
    /// Observed cross-file fact signature captured at cold-compute
    /// publish time. Stored as `Arc<[FactVersionRef]>` so warm-hit
    /// validation clones the handle without copying the slice
    /// (R3/R26/R28 fact-validation substrate).
    pub fact_versions: Arc<[crate::resolver_core::FactVersionRef]>,
    pub payload: Vec<u8>,
    /// `project_generation` captured at publish — the value-side
    /// generation backstop, the same discipline as the typed result
    /// caches' `validated_at_generation` gate. The payload lane has no
    /// outer publish / `is_stable` fence, so an UNDER-RECORDED fact
    /// signature (degenerate case: the empty signature, which validates
    /// trivially) would otherwise keep validating across project-shape
    /// mutations permanently. A warm hit demands the LIVE generation.
    pub validated_at_generation: u64,
}

// ═══════════════════════════════════════════════════════════════════════════
// MetaProvenance — per-host counters for component-meta observability
// ═══════════════════════════════════════════════════════════════════════════

/// Number of [`crate::semantic_query::SemanticNodeData`] discriminants used
/// to size the per-discriminant push-count array in [`MetaProvenance`].
///
/// Sized with headroom over the current variants so adding a
/// variant doesn't require widening the array. If
/// `SemanticNodeData::discriminant_index` ever returns `>= 32`, that's
/// a debug-assert hit at the push site rather than a silent overflow.
pub const SEMANTIC_NODE_DATA_DISCRIMINANT_COUNT: usize = 32;

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
    /// `tests/cases/g_session/session_view_isolation.rs` to assert the R17
    /// invariant that session query paths do NOT mutate the host.
    pub host_upsert_calls: std::sync::atomic::AtomicU64,
    /// Bumped on every cache-key derivation that consulted a
    /// [`crate::session_view::SessionView`] via
    /// `view.content_hash_for(canonical)` rather than the base host's
    /// `shallow_file_state(canonical).whole_hash`. Used by
    /// `tests/cases/g_session/session_view_warm_reuse.rs` to assert R17/R18 (the
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
    /// Count of session-overlay RE-ROOTS performed against THIS host —
    /// incremented once per
    /// [`crate::resolver_store::HostStoreView::with_session_overlay`] call
    /// that reaches the `Arc::make_mut` re-root path (a non-empty overlay
    /// or tombstone set). `Arc::make_mut` clones the shared
    /// `StoreViewSnapshot` only when the `Arc` is actually shared
    /// (refcount > 1); a uniquely-owned snapshot is mutated in place — so
    /// this counter is an UPPER BOUND on full snapshot clones, counting
    /// every entry into the re-root work (clone or in-place), which is
    /// exactly the per-application cost the O(1) batch contract bounds. A
    /// no-op (empty-overlay) application keeps the shared base `Arc`
    /// untouched and does NOT bump this counter.
    ///
    /// PER-HOST (not process-global): every `with_session_overlay` call
    /// already carries the `&VerterHost` it overlays, and every rayon
    /// worker in a host batch operates on the SAME host, so this counter
    /// observes worker-side per-job COWs while staying immune to other
    /// hosts' (other tests') overlay activity. A component-meta batch over
    /// an overlay session must apply the overlay ONCE per batch (the
    /// per-batch capture) and SHARE it across all N jobs, so a warm or
    /// cold batch of N performs O(1) overlay COWs on this host; a per-job
    /// re-application drives it O(N) — the regression
    /// `batch_over_overlay_session_applies_overlay_o1_not_per_job` gates
    /// against.
    pub session_overlay_cows: std::sync::atomic::AtomicU64,
    /// Count of FULL overlay-set fingerprint computations performed
    /// against THIS host — incremented once per
    /// [`crate::session_view::overlay_set_fingerprint`] call that walks
    /// the overlay-hash table (collect + sort by canonical + FxHash). An
    /// overlay-bearing view's
    /// [`crate::session_view::SessionView::fingerprint`] is a PURE
    /// function of the view's immutable overlay maps, so the full
    /// computation runs ONCE — at view construction — and every later
    /// `fingerprint()` read returns the memoized `u64` with no recompute.
    ///
    /// PER-HOST (not process-global): an overlay view always carries the
    /// `&VerterHost` it overlays, and every rayon worker in a host batch
    /// reads the SAME shared view's memoized fingerprint, so this counter
    /// observes worker-side reads while staying immune to other hosts'
    /// (other tests') fingerprinting. A component-meta batch over an
    /// overlay session constructs ONE view per batch and shares it across
    /// all N jobs, so a warm or cold batch of N performs O(1) full
    /// fingerprint computations on this host; recomputing per `cache_key`
    /// / per warm-probe / per store would drive it O(N) — the regression
    /// `batch_over_overlay_session_computes_fingerprint_o1_not_per_job`
    /// gates against.
    pub overlay_set_fingerprint_full_computations: std::sync::atomic::AtomicU64,
    /// Count of [`crate::resolver_store::HostStoreView::from_host_read`]
    /// entries against THIS host — every store-view read a warm-cache
    /// validator or batch capture performs, whether the manager serves it
    /// as a cheap token-stable `Arc` clone or a full sweep.
    ///
    /// PER-HOST (not process-global): `from_host_read` already carries the
    /// `&VerterHost` it reads, and every rayon worker in a host batch reads
    /// through the SAME host, so this counter observes worker-side per-job
    /// reads while staying immune to other hosts' (other tests')
    /// store-view traffic. A warm component-meta batch of N must collapse
    /// onto O(1) reads (the single per-batch fixed-view capture); a
    /// per-job-read path drives it ≥ N — the regressions
    /// `warm_batch_payload_from_host_calls_are_o1_not_per_item` /
    /// `warm_analysis_batch_from_host_calls_are_o1_not_per_item` gate
    /// against. The process-global per-call-site table
    /// (`dump_from_host_call_sites`) remains the bench-side ATTRIBUTION
    /// diagnostic; this counter is the hermetic per-host MEASUREMENT.
    pub store_view_from_host_reads: std::sync::atomic::AtomicU64,
    /// `ComponentMetaResultDb::get_with_view` warm-hit count. Bumped
    /// once per call that returns `Some(entry)` after the entry's
    /// `fact_dep_signature` validates under the supplied
    /// [`crate::resolver_core::StoreView`]. Used by behavioural
    /// tests to discriminate fact-validation from eager-invalidation:
    /// an entry that survives an unrelated edit must advance this
    /// counter on the second call.
    pub component_meta_result_cache_hits: std::sync::atomic::AtomicU64,
    /// `ComponentMetaResultDb::get_with_view` miss count. Bumped on
    /// every call that returns `None` — whether the entry was absent
    /// from the map OR the entry's `fact_dep_signature` failed
    /// validation. Used by tests to discriminate cache-bypass via
    /// the validator: editing a dep MUST advance this counter on
    /// the second call.
    pub component_meta_result_cache_misses: std::sync::atomic::AtomicU64,
    /// Count of dispatch fact fan-outs emitted from the slot-binding graph.
    /// The traversal has no cache boundary of its own, so behavioral tests use
    /// this counter to prove its dependency evidence reached the request tracer.
    pub slot_binding_graph_fact_tracer_emissions: std::sync::atomic::AtomicU64,
    /// Count of `observe_fact_signature` fan-out calls emitted from
    /// `meta_resolve::dep_signature::emit_dispatch_dep_signature_facts`
    /// — the helper invoked by dispatch reads that
    /// have no result cache of their own (three projector sites,
    /// `materialize_component_meta_type_expr_until_stable_full`,
    /// `node_root_reaches_transitive_cycle_with_fence`, and
    /// `materialize_member_surface_expr`). The helper bumps this
    /// counter on every `observe_fact_signature` call.
    pub dispatch_dep_signature_fact_tracer_emissions: std::sync::atomic::AtomicU64,
    pub indexed_ready_scheduler_snapshot_reuse: std::sync::atomic::AtomicU64,
    pub bundle_cache_hits: std::sync::atomic::AtomicU64,
    /// Request-scoped session-overlay prepared-decl bundle memo hits —
    /// bumped when `prepared_decl_bundle_with_context` serves an
    /// overlay-bearing bundle from the request's
    /// `CanonicalCompletionOverlay` memo instead of re-running
    /// `materialize_prepared_decl_bundle_via_ctx` (R17 keeps that bundle
    /// out of the shared `prepared_decl_bundles` cache, so this memo is
    /// its only reuse tier). The sibling of `bundle_cache_hits` for the
    /// overlay path; the end-to-end wiring regression asserts it moves
    /// under a real session-view component-meta request.
    pub overlay_bundle_memo_hits: std::sync::atomic::AtomicU64,
    pub bundle_materializations: std::sync::atomic::AtomicU64,
    /// Cold bundle flight-body executions: the singleflight lane's cold
    /// run past the in-flight recheck (the deterministic mirror of
    /// `AuditEvent::PreparedDeclBundleCold`). Joiners adopting a
    /// retained rendezvous do not count — the adopt-vs-rerun
    /// discriminator for miss-retention tests, where a surface-empty
    /// re-run bumps NO materialisation counter (the producers conclude
    /// the miss before building anything).
    pub bundle_cold_flight_runs: std::sync::atomic::AtomicU64,
    pub dep_resolution_calls: std::sync::atomic::AtomicU64,
    pub imported_macro_declaration_builds: std::sync::atomic::AtomicU64,
    /// Cold compile-output computes: bumped exactly once per cold run of
    /// `ensure_compile_artifacts` (the path PAST the warm-hit consult, where
    /// the shared compile actually executes). The deterministic, feature-
    /// independent observability rail for compile-slot COALESCING: two
    /// concurrent requests on the SAME `(canonical, profile)` that coalesce
    /// onto one shared compile bump this ONCE; two independent compiles bump it
    /// twice. (The `session_metrics` `compile_requests` counter mirrors this
    /// but is feature-gated; this `MetaProvenance` rail is always on, like the
    /// cold per-file artifact-build dedup counters below.) `reset()` zeroes it.
    pub compile_cold_runs: std::sync::atomic::AtomicU64,

    // ── Cold per-file artifact-build dedup counters ─────────────────────
    //
    // One cold resolve of one canonical performs exactly ONE of each:
    // one eval-program parse, one eval-env build, one shallow-state
    // build, one `IndexedReady` materialisation. These counters are the
    // deterministic observability rail for that contract (no
    // wall-clock); `reset()` zeroes them like every other counter.
    /// OXC eval-program parses performed through the single host parse
    /// entry (`parse_eval_program`). Exactly 1 per cold canonical build.
    pub eval_program_parses: std::sync::atomic::AtomicU64,
    /// Carrier parses performed through the single counted carrier
    /// chokepoint (`parse::parse_carrier_counted`) — every framework
    /// carrier (`.vue`, `.svelte`, …) increments this exactly once per
    /// `CarrierCompiler::parse`. The framework-neutral parse-once rail:
    /// a cold build of any carrier file bumps this once, so a duplicate
    /// carrier parse on any host lane (Vue OR Svelte) is counter-visible
    /// without naming a framework.
    pub carrier_parses: std::sync::atomic::AtomicU64,
    /// SFC structure parses (the Vue carrier compatibility rail) —
    /// bumped by `parse::parse_carrier_counted` only when the dispatched
    /// carrier is Vue, covering the materialise lanes (base + overlay),
    /// the compile/template merged-source lanes, and the lazy
    /// `get_analysis` re-parse fallbacks, so a duplicate-SFC-parse
    /// regression on any Vue host lane stays counter-visible alongside
    /// the neutral `carrier_parses` rail.
    pub sfc_parses: std::sync::atomic::AtomicU64,
    /// Full OXC program parses through `parse_non_sfc_snapshot` —
    /// the scheduler snapshot lane for non-SFC files plus the
    /// `build_snapshot_from_source` analysis read path. Distinct from
    /// `eval_program_parses`
    /// (the `parse_eval_program` funnel) and `sfc_parses` (SFC
    /// structure parses); counted inside the worker fn so every lane
    /// counts.
    pub non_sfc_snapshot_parses: std::sync::atomic::AtomicU64,
    /// Full OXC SCRIPT-program parses on the `.vue` snapshot path —
    /// the position-preserving script source extracted from the SFC and
    /// parsed for export signatures + script analysis. Exactly 1 per
    /// `.vue` snapshot build: both consumers walk the SAME program (the
    /// `_from_program` threading), so a count of 2 on one snapshot
    /// build means a lane re-introduced a per-consumer re-parse of the
    /// same script bytes. Distinct from `sfc_parses` (the SFC STRUCTURE
    /// parse, not an OXC program parse) and `eval_program_parses` (the
    /// eval funnel); counted inside the worker fn so every lane counts.
    pub vue_script_snapshot_parses: std::sync::atomic::AtomicU64,
    /// `EvalEnv` builds initiated by the host (the program-taking
    /// builder plus any call site that forces an internal fallback
    /// build). Exactly 1 per cold canonical build.
    pub eval_env_builds: std::sync::atomic::AtomicU64,
    /// Declaration BODIES lowered to typed IR on behalf of this host —
    /// one increment per type/value/augmentation declaration contributor
    /// whose body (annotation, signature set, object shape, heritage,
    /// member types) was lowered from OXC syntax. The deterministic
    /// demand-scoping rail: publishing a file's `IndexedReady` lowers
    /// ZERO bodies; a semantic query lowers exactly the demanded
    /// declaration closure; a whole-file env demand (fallthrough /
    /// runtime values) lowers the file's full declaration set once.
    pub decl_bodies_lowered: std::sync::atomic::AtomicU64,
    /// `ShallowFileState::from_analysis_with_resolver` builds initiated
    /// by host call sites. Exactly 1 per cold canonical build.
    pub shallow_state_builds: std::sync::atomic::AtomicU64,
    /// Cold `IndexedReady` materialisations (base + overlay
    /// materialiser bodies). Exactly 1 per cold canonical build.
    pub indexed_ready_materializes: std::sync::atomic::AtomicU64,
    /// Route-surface edge refreshes that reused the content-addressed
    /// `IndexedReady` payload (no re-parse) and rebuilt only the route
    /// surface after a route-resolution mutation.
    pub indexed_ready_edge_refreshes: std::sync::atomic::AtomicU64,

    // ── Contention instrumentation ──────────────────────────────────────
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
    /// (equal to `node_arena_pushes` while every push allocates; diverges
    /// once structural interning serves a push from an existing slot).
    pub node_arena_intern_miss: std::sync::atomic::AtomicU64,
    /// Time spent waiting on `ArenaInner` mutex acquisition during pushes
    /// (lock-contention observability counter).
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

    // ── Family B/C/D producer-install observability ───────────
    //
    // `install_fact_tracer` substrate counters for the 5 caches wired
    // through the producer-install observability surface. Each cache
    // exposes two counters:
    //
    // - `<cache>_fact_tracer_installs` — number of cold-compute calls
    //   wrapped in `install_fact_tracer` (advances once per cold
    //   producer entry).
    // - `<cache>_overflow_refusals` — number of cold-compute calls
    //   whose observation set exceeded `FACT_SIGNATURE_CAP` (1024) and
    //   were therefore NOT admitted to the warm cache (caller
    //   cold-recomputes on next request).
    //
    // Caches: `MaterializeStructureDb`, `RefCycleResultDb`,
    // `MemoEntry`, `AppConfigNoOverrideProofDb`, `OwnerImportSurfaceDb`.
    /// `install_fact_tracer` wrap count for `MaterializeStructureDb`.
    pub materialize_structure_fact_tracer_installs: std::sync::atomic::AtomicU64,
    /// `install_fact_tracer` overflow-refusal count for `MaterializeStructureDb`.
    pub materialize_structure_overflow_refusals: std::sync::atomic::AtomicU64,
    /// `install_fact_tracer` wrap count for `RefCycleResultDb`.
    pub ref_cycle_fact_tracer_installs: std::sync::atomic::AtomicU64,
    /// `install_fact_tracer` overflow-refusal count for `RefCycleResultDb`.
    pub ref_cycle_overflow_refusals: std::sync::atomic::AtomicU64,
    /// `install_fact_tracer` wrap count for `MemoEntry` (semantic
    /// query memo cold builds).
    pub memo_entry_fact_tracer_installs: std::sync::atomic::AtomicU64,
    /// `install_fact_tracer` overflow-refusal count for `MemoEntry`.
    pub memo_entry_overflow_refusals: std::sync::atomic::AtomicU64,
    /// `install_fact_tracer` wrap count for `AppConfigNoOverrideProofDb`.
    pub app_config_proof_fact_tracer_installs: std::sync::atomic::AtomicU64,
    /// `install_fact_tracer` overflow-refusal count for
    /// `AppConfigNoOverrideProofDb`.
    pub app_config_proof_overflow_refusals: std::sync::atomic::AtomicU64,
    /// `install_fact_tracer` wrap count for `OwnerImportSurfaceDb`.
    pub owner_import_surface_fact_tracer_installs: std::sync::atomic::AtomicU64,
    /// `install_fact_tracer` overflow-refusal count for
    /// `OwnerImportSurfaceDb`.
    pub owner_import_surface_overflow_refusals: std::sync::atomic::AtomicU64,
    /// Admission refusals for `OwnerImportSurfaceDb` because an
    /// unresolved direct import could not be rooted in the owner's
    /// `ImportRoute` fact rail (no coverage for the skipped specifier).
    /// The surface is served to the caller but never cached — the next
    /// request cold-recomputes against the live workspace.
    pub owner_import_surface_unrooted_skip_refusals: std::sync::atomic::AtomicU64,
    /// Admission refusals for `OwnerImportSurfaceDb` because the cold
    /// build consumed a FENCED (ReturnOnly) serve — either the traced
    /// scope observed one by value, or a per-binding route walk
    /// returned the strict-admission empty-facts signal. The surface
    /// is served to the caller but never cached; the next request
    /// cold-recomputes against the live workspace.
    pub owner_import_surface_fenced_serve_refusals: std::sync::atomic::AtomicU64,
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
            session_overlay_cows: std::sync::atomic::AtomicU64::new(0),
            overlay_set_fingerprint_full_computations: std::sync::atomic::AtomicU64::new(0),
            store_view_from_host_reads: std::sync::atomic::AtomicU64::new(0),
            component_meta_result_cache_hits: std::sync::atomic::AtomicU64::new(0),
            component_meta_result_cache_misses: std::sync::atomic::AtomicU64::new(0),
            slot_binding_graph_fact_tracer_emissions: std::sync::atomic::AtomicU64::new(0),
            dispatch_dep_signature_fact_tracer_emissions: std::sync::atomic::AtomicU64::new(0),
            indexed_ready_scheduler_snapshot_reuse: std::sync::atomic::AtomicU64::new(0),
            bundle_cache_hits: std::sync::atomic::AtomicU64::new(0),
            overlay_bundle_memo_hits: std::sync::atomic::AtomicU64::new(0),
            bundle_materializations: std::sync::atomic::AtomicU64::new(0),
            bundle_cold_flight_runs: std::sync::atomic::AtomicU64::new(0),
            dep_resolution_calls: std::sync::atomic::AtomicU64::new(0),
            imported_macro_declaration_builds: std::sync::atomic::AtomicU64::new(0),
            compile_cold_runs: std::sync::atomic::AtomicU64::new(0),
            eval_program_parses: std::sync::atomic::AtomicU64::new(0),
            carrier_parses: std::sync::atomic::AtomicU64::new(0),
            sfc_parses: std::sync::atomic::AtomicU64::new(0),
            non_sfc_snapshot_parses: std::sync::atomic::AtomicU64::new(0),
            vue_script_snapshot_parses: std::sync::atomic::AtomicU64::new(0),
            eval_env_builds: std::sync::atomic::AtomicU64::new(0),
            decl_bodies_lowered: std::sync::atomic::AtomicU64::new(0),
            shallow_state_builds: std::sync::atomic::AtomicU64::new(0),
            indexed_ready_materializes: std::sync::atomic::AtomicU64::new(0),
            indexed_ready_edge_refreshes: std::sync::atomic::AtomicU64::new(0),
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
            materialize_structure_fact_tracer_installs: std::sync::atomic::AtomicU64::new(0),
            materialize_structure_overflow_refusals: std::sync::atomic::AtomicU64::new(0),
            ref_cycle_fact_tracer_installs: std::sync::atomic::AtomicU64::new(0),
            ref_cycle_overflow_refusals: std::sync::atomic::AtomicU64::new(0),
            memo_entry_fact_tracer_installs: std::sync::atomic::AtomicU64::new(0),
            memo_entry_overflow_refusals: std::sync::atomic::AtomicU64::new(0),
            app_config_proof_fact_tracer_installs: std::sync::atomic::AtomicU64::new(0),
            app_config_proof_overflow_refusals: std::sync::atomic::AtomicU64::new(0),
            owner_import_surface_fact_tracer_installs: std::sync::atomic::AtomicU64::new(0),
            owner_import_surface_overflow_refusals: std::sync::atomic::AtomicU64::new(0),
            owner_import_surface_unrooted_skip_refusals: std::sync::atomic::AtomicU64::new(0),
            owner_import_surface_fenced_serve_refusals: std::sync::atomic::AtomicU64::new(0),
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
                "session_overlay_cows",
                &self.session_overlay_cows.load(Relaxed),
            )
            .field(
                "overlay_set_fingerprint_full_computations",
                &self.overlay_set_fingerprint_full_computations.load(Relaxed),
            )
            .field(
                "store_view_from_host_reads",
                &self.store_view_from_host_reads.load(Relaxed),
            )
            .field(
                "indexed_ready_scheduler_snapshot_reuse",
                &self.indexed_ready_scheduler_snapshot_reuse.load(Relaxed),
            )
            .field("bundle_cache_hits", &self.bundle_cache_hits.load(Relaxed))
            .field(
                "overlay_bundle_memo_hits",
                &self.overlay_bundle_memo_hits.load(Relaxed),
            )
            .field(
                "bundle_materializations",
                &self.bundle_materializations.load(Relaxed),
            )
            .field(
                "bundle_cold_flight_runs",
                &self.bundle_cold_flight_runs.load(Relaxed),
            )
            .field(
                "dep_resolution_calls",
                &self.dep_resolution_calls.load(Relaxed),
            )
            .field(
                "imported_macro_declaration_builds",
                &self.imported_macro_declaration_builds.load(Relaxed),
            )
            .field("compile_cold_runs", &self.compile_cold_runs.load(Relaxed))
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
            session_overlay_cows: self.session_overlay_cows.load(Relaxed),
            overlay_set_fingerprint_full_computations: self
                .overlay_set_fingerprint_full_computations
                .load(Relaxed),
            store_view_from_host_reads: self.store_view_from_host_reads.load(Relaxed),
            component_meta_result_cache_hits: self.component_meta_result_cache_hits.load(Relaxed),
            component_meta_result_cache_misses: self
                .component_meta_result_cache_misses
                .load(Relaxed),
            slot_binding_graph_fact_tracer_emissions: self
                .slot_binding_graph_fact_tracer_emissions
                .load(Relaxed),
            dispatch_dep_signature_fact_tracer_emissions: self
                .dispatch_dep_signature_fact_tracer_emissions
                .load(Relaxed),
            indexed_ready_scheduler_snapshot_reuse: self
                .indexed_ready_scheduler_snapshot_reuse
                .load(Relaxed),
            bundle_cache_hits: self.bundle_cache_hits.load(Relaxed),
            overlay_bundle_memo_hits: self.overlay_bundle_memo_hits.load(Relaxed),
            bundle_materializations: self.bundle_materializations.load(Relaxed),
            bundle_cold_flight_runs: self.bundle_cold_flight_runs.load(Relaxed),
            dep_resolution_calls: self.dep_resolution_calls.load(Relaxed),
            imported_macro_declaration_builds: self.imported_macro_declaration_builds.load(Relaxed),
            compile_cold_runs: self.compile_cold_runs.load(Relaxed),
            eval_program_parses: self.eval_program_parses.load(Relaxed),
            carrier_parses: self.carrier_parses.load(Relaxed),
            sfc_parses: self.sfc_parses.load(Relaxed),
            non_sfc_snapshot_parses: self.non_sfc_snapshot_parses.load(Relaxed),
            vue_script_snapshot_parses: self.vue_script_snapshot_parses.load(Relaxed),
            eval_env_builds: self.eval_env_builds.load(Relaxed),
            decl_bodies_lowered: self.decl_bodies_lowered.load(Relaxed),
            shallow_state_builds: self.shallow_state_builds.load(Relaxed),
            indexed_ready_materializes: self.indexed_ready_materializes.load(Relaxed),
            indexed_ready_edge_refreshes: self.indexed_ready_edge_refreshes.load(Relaxed),
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
            materialize_structure_fact_tracer_installs: self
                .materialize_structure_fact_tracer_installs
                .load(Relaxed),
            materialize_structure_overflow_refusals: self
                .materialize_structure_overflow_refusals
                .load(Relaxed),
            ref_cycle_fact_tracer_installs: self.ref_cycle_fact_tracer_installs.load(Relaxed),
            ref_cycle_overflow_refusals: self.ref_cycle_overflow_refusals.load(Relaxed),
            memo_entry_fact_tracer_installs: self.memo_entry_fact_tracer_installs.load(Relaxed),
            memo_entry_overflow_refusals: self.memo_entry_overflow_refusals.load(Relaxed),
            app_config_proof_fact_tracer_installs: self
                .app_config_proof_fact_tracer_installs
                .load(Relaxed),
            app_config_proof_overflow_refusals: self
                .app_config_proof_overflow_refusals
                .load(Relaxed),
            owner_import_surface_fact_tracer_installs: self
                .owner_import_surface_fact_tracer_installs
                .load(Relaxed),
            owner_import_surface_overflow_refusals: self
                .owner_import_surface_overflow_refusals
                .load(Relaxed),
            owner_import_surface_unrooted_skip_refusals: self
                .owner_import_surface_unrooted_skip_refusals
                .load(Relaxed),
            owner_import_surface_fenced_serve_refusals: self
                .owner_import_surface_fenced_serve_refusals
                .load(Relaxed),
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
        self.session_overlay_cows.store(0, Relaxed);
        self.overlay_set_fingerprint_full_computations
            .store(0, Relaxed);
        self.store_view_from_host_reads.store(0, Relaxed);
        self.component_meta_result_cache_hits.store(0, Relaxed);
        self.component_meta_result_cache_misses.store(0, Relaxed);
        self.slot_binding_graph_fact_tracer_emissions
            .store(0, Relaxed);
        self.dispatch_dep_signature_fact_tracer_emissions
            .store(0, Relaxed);
        self.indexed_ready_scheduler_snapshot_reuse
            .store(0, Relaxed);
        self.bundle_cache_hits.store(0, Relaxed);
        self.overlay_bundle_memo_hits.store(0, Relaxed);
        self.bundle_materializations.store(0, Relaxed);
        self.bundle_cold_flight_runs.store(0, Relaxed);
        self.dep_resolution_calls.store(0, Relaxed);
        self.imported_macro_declaration_builds.store(0, Relaxed);
        self.compile_cold_runs.store(0, Relaxed);
        self.eval_program_parses.store(0, Relaxed);
        self.carrier_parses.store(0, Relaxed);
        self.sfc_parses.store(0, Relaxed);
        self.non_sfc_snapshot_parses.store(0, Relaxed);
        self.vue_script_snapshot_parses.store(0, Relaxed);
        self.eval_env_builds.store(0, Relaxed);
        self.decl_bodies_lowered.store(0, Relaxed);
        self.shallow_state_builds.store(0, Relaxed);
        self.indexed_ready_materializes.store(0, Relaxed);
        self.indexed_ready_edge_refreshes.store(0, Relaxed);
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
        self.materialize_structure_fact_tracer_installs
            .store(0, Relaxed);
        self.materialize_structure_overflow_refusals
            .store(0, Relaxed);
        self.ref_cycle_fact_tracer_installs.store(0, Relaxed);
        self.ref_cycle_overflow_refusals.store(0, Relaxed);
        self.memo_entry_fact_tracer_installs.store(0, Relaxed);
        self.memo_entry_overflow_refusals.store(0, Relaxed);
        self.app_config_proof_fact_tracer_installs.store(0, Relaxed);
        self.app_config_proof_overflow_refusals.store(0, Relaxed);
        self.owner_import_surface_fact_tracer_installs
            .store(0, Relaxed);
        self.owner_import_surface_overflow_refusals
            .store(0, Relaxed);
        self.owner_import_surface_unrooted_skip_refusals
            .store(0, Relaxed);
        self.owner_import_surface_fenced_serve_refusals
            .store(0, Relaxed);
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
    /// Session-overlay re-roots performed against this host (one bump per
    /// entry into the `Arc::make_mut` re-root path in
    /// [`crate::resolver_store::HostStoreView::with_session_overlay`] —
    /// an upper bound on actual snapshot clones, since a uniquely-owned
    /// snapshot is mutated in place; no bump on a no-op empty-overlay
    /// application). Per-host, so it observes worker-side per-job re-roots
    /// in a batch while staying isolated from other hosts' overlay
    /// activity.
    pub session_overlay_cows: u64,
    /// Full overlay-set fingerprint computations performed against this
    /// host (one bump per [`crate::session_view::overlay_set_fingerprint`]
    /// call that walks the overlay-hash table — collect + sort + hash —
    /// NOT per memoized `fingerprint()` read). Per-host, so it observes
    /// worker-side reads in a batch while staying isolated from other
    /// hosts' fingerprinting.
    pub overlay_set_fingerprint_full_computations: u64,
    /// [`crate::resolver_store::HostStoreView::from_host_read`] entries
    /// against this host — every store-view read (manager `Arc`-clone hit
    /// or full sweep alike). Per-host, so it observes worker-side per-job
    /// reads in a batch while staying isolated from other hosts'
    /// store-view traffic.
    pub store_view_from_host_reads: u64,
    pub component_meta_result_cache_hits: u64,
    pub component_meta_result_cache_misses: u64,
    /// Per-call count of dispatch fact fan-outs emitted from
    /// `meta_resolve/slot_binding_graph.rs`.
    pub slot_binding_graph_fact_tracer_emissions: u64,
    /// Per-call count of `observe_fact_signature` fan-outs emitted
    /// from the six dispatch-read sites that route through
    /// `meta_resolve::dep_signature::emit_dispatch_dep_signature_facts`
    /// (three projector sites,
    /// `materialize_component_meta_type_expr_until_stable_full`,
    /// `node_root_reaches_transitive_cycle_with_fence`, and
    /// `materialize_member_surface_expr`). Used by behavioural tests
    /// to discriminate the fact-tracer path from the legacy
    /// request-tracer path.
    pub dispatch_dep_signature_fact_tracer_emissions: u64,
    pub indexed_ready_scheduler_snapshot_reuse: u64,
    pub bundle_cache_hits: u64,
    pub overlay_bundle_memo_hits: u64,
    pub bundle_materializations: u64,
    pub bundle_cold_flight_runs: u64,
    pub dep_resolution_calls: u64,
    pub imported_macro_declaration_builds: u64,
    pub compile_cold_runs: u64,
    pub eval_program_parses: u64,
    pub carrier_parses: u64,
    pub sfc_parses: u64,
    pub non_sfc_snapshot_parses: u64,
    pub vue_script_snapshot_parses: u64,
    pub eval_env_builds: u64,
    pub decl_bodies_lowered: u64,
    pub shallow_state_builds: u64,
    pub indexed_ready_materializes: u64,
    pub indexed_ready_edge_refreshes: u64,
    /// Contention instrumentation counters surfaced through the
    /// host's `MetaProvenance`.
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

    // ── Family B/C/D producer-install observability ───────────
    /// `install_fact_tracer` wrap count for `MaterializeStructureDb`.
    pub materialize_structure_fact_tracer_installs: u64,
    /// `install_fact_tracer` overflow-refusal count for `MaterializeStructureDb`.
    pub materialize_structure_overflow_refusals: u64,
    /// `install_fact_tracer` wrap count for `RefCycleResultDb`.
    pub ref_cycle_fact_tracer_installs: u64,
    /// `install_fact_tracer` overflow-refusal count for `RefCycleResultDb`.
    pub ref_cycle_overflow_refusals: u64,
    /// `install_fact_tracer` wrap count for `MemoEntry`.
    pub memo_entry_fact_tracer_installs: u64,
    /// `install_fact_tracer` overflow-refusal count for `MemoEntry`.
    pub memo_entry_overflow_refusals: u64,
    /// `install_fact_tracer` wrap count for `AppConfigNoOverrideProofDb`.
    pub app_config_proof_fact_tracer_installs: u64,
    /// `install_fact_tracer` overflow-refusal count for `AppConfigNoOverrideProofDb`.
    pub app_config_proof_overflow_refusals: u64,
    /// `install_fact_tracer` wrap count for `OwnerImportSurfaceDb`.
    pub owner_import_surface_fact_tracer_installs: u64,
    /// `install_fact_tracer` overflow-refusal count for `OwnerImportSurfaceDb`.
    pub owner_import_surface_overflow_refusals: u64,
    pub owner_import_surface_unrooted_skip_refusals: u64,
    pub owner_import_surface_fenced_serve_refusals: u64,
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
    // HostConfig default tests
    // -----------------------------------------------------------------------

    /// The external-resolution step budget defaults to `None` so the
    /// production frontier construction site keeps using
    /// `MAX_EXTERNAL_TYPE_RESOLVE_STEPS`. A non-`None` default would
    /// silently re-cap every production resolution.
    #[test]
    fn host_config_default_external_resolution_step_budget_is_none() {
        assert_eq!(HostConfig::default().external_resolution_step_budget, None);
    }

    // -----------------------------------------------------------------------
    // PoolSize::resolve totality
    // -----------------------------------------------------------------------

    /// `PoolSize::Fraction` is a public field-struct variant, so a caller can
    /// pass a malformed `{ divisor: 0, min: 4, max: 1 }` (zero divisor AND
    /// inverted bounds). `resolve()` must be TOTAL: it must NOT panic — a
    /// `min > max` would trip `clamp`'s `min <= max` requirement and a zero
    /// divisor would divide-by-zero — and must land inside the caller's
    /// ORDERED bounds, never below 1. Reverting the ordering / divisor floor
    /// makes the inverted-bounds cases panic (RED).
    #[test]
    fn pool_size_fraction_resolve_is_total_on_malformed_bounds() {
        // Zero divisor + inverted bounds: must not panic; ordered bounds are
        // [min(4,1), max(4,1)] == [1, 4].
        let resolved = PoolSize::Fraction {
            divisor: 0,
            min: 4,
            max: 1,
        }
        .resolve();
        assert!(
            (1..=4).contains(&resolved),
            "malformed Fraction must resolve in the ordered bounds [1, 4], got {resolved}"
        );

        // Inverted bounds with a valid divisor — isolates the `min > max`
        // clamp panic from the divisor guard. Ordered bounds [2, 8].
        let inverted = PoolSize::Fraction {
            divisor: 4,
            min: 8,
            max: 2,
        }
        .resolve();
        assert!(
            (2..=8).contains(&inverted),
            "inverted-bounds Fraction must resolve in [2, 8], got {inverted}"
        );

        // A well-formed Fraction still resolves exactly as before the fix:
        // (available_parallelism / 4) clamped to [1, 4]. The ordering /
        // fallback-clamp must NOT change well-formed resolution, so this
        // equals the decl-lowering default sizing.
        let normal = PoolSize::Fraction {
            divisor: 4,
            min: 1,
            max: 4,
        }
        .resolve();
        assert!(
            (1..=4).contains(&normal),
            "well-formed Fraction must resolve in [1, 4], got {normal}"
        );
        assert_eq!(
            normal,
            DECL_LOWERING_DEFAULT_POOL_SIZE.resolve(),
            "well-formed Fraction resolution must be unchanged by the totality fix"
        );
    }

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
