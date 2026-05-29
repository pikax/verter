//! Compile-cache mode classifier.
//!
//! Decides which [`crate::CompileCacheMode`] a compile request runs
//! under, given the requested mode, the compile input, the resolver
//! context, and the host configuration. The classifier is the SOLE
//! authority for the mode decision — [`crate::host_resolve`] consumes
//! the classification and routes by the resulting `actual_mode`.
//!
//! ## Eligibility predicates
//!
//! Each downgrade reason has a small, testable eligibility predicate
//! ([`has_external_src`], [`has_macro_type_deps`],
//! [`has_workspace_alias`], [`has_module_augmentation`],
//! [`has_block_override`], [`has_style_override`],
//! [`has_ide_only_analysis`], [`has_dev_last_good`]). The composite
//! [`classify_compile_mode`] walks them in deterministic priority
//! order, collects every triggering reason into the
//! [`CompileModeClassification::downgrade_reasons`] vector, and folds
//! the requested mode down to the most-cache-rich mode the inputs
//! support.
//!
//! ## Priority order (CRITICAL)
//!
//! Reasons are collected in this order so the public single-field
//! `Option<DowngradeReason>` projection is deterministic:
//!
//! `HasModuleAugmentation` then `HasMacroTypeDeps` then
//! `HasWorkspaceAlias` then `HasExternalSrc` then `HasBlockOverride`
//! then `HasStyleOverride` then `HasIdeOnlyAnalysis` then
//! `HasDevLastGood`.
//!
//! ## Mode-downgrade ladder
//!
//! [`CompileCacheMode::Session`] can downgrade to
//! [`CompileCacheMode::Content`] or [`CompileCacheMode::Stateless`].
//! [`CompileCacheMode::Content`] can downgrade to
//! [`CompileCacheMode::Stateless`]. [`CompileCacheMode::Stateless`]
//! never downgrades — it is the floor and ignores every reason
//! (stateless already bypasses host caches).
//!
//! `#![allow(dead_code)]` at module scope: the helpers and classifier
//! land ahead of the compile-entry-path wiring that consumes them.
//! The inline `tests` module exercises every public surface
//! independently of the routing.
#![allow(dead_code)]

use smallvec::SmallVec;
use verter_workspace::WorkspaceAlias;

use crate::types::{
    CompileCacheMode, CompileErrorPolicy, CompileInput, CompileProfile, DowngradeReason, HostConfig,
};

/// Final classification produced by [`classify_compile_mode`].
///
/// `actual_mode` is what the runtime routes through. `downgrade_reasons`
/// captures every triggering reason in priority order; the public
/// single-reason projection on the compile result is
/// `downgrade_reasons.first()` (the highest-priority reason). The full
/// vector lands on the [`verter_audit::StructuredAuditEvent::CompileModeDowngrade`]
/// payload for telemetry.
///
/// `pub(crate)` because the classification is an implementation
/// detail of the in-crate compile entry path. The public compile
/// result surface ([`crate::types::CompileBatchEntry`]) carries the
/// projected `actual_mode` + `Option<DowngradeReason>` fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CompileModeClassification {
    /// The mode the caller asked for.
    pub(crate) requested_mode: CompileCacheMode,
    /// The mode the runtime actually runs under (after downgrade).
    pub(crate) actual_mode: CompileCacheMode,
    /// All triggering reasons in priority order. Empty when
    /// `actual_mode == requested_mode`.
    pub(crate) downgrade_reasons: SmallVec<[DowngradeReason; 2]>,
}

impl CompileModeClassification {
    /// The highest-priority single downgrade reason, when any reason
    /// triggered. Used by the public single-field projection on the
    /// compile result type.
    pub(crate) fn first_downgrade_reason(&self) -> Option<DowngradeReason> {
        self.downgrade_reasons.first().copied()
    }
}

/// True iff the compile input references external `src="..."` blocks.
/// External blocks resolve outside the env-hash dimensions, so a
/// pure-content key cannot key on them.
#[inline]
pub(crate) fn has_external_src(input: &CompileInput) -> bool {
    !input.external_requests.is_empty()
}

/// True iff the compile input has macro type dependencies. Macro-
/// resolved types depend on cross-file type traversal that lives
/// outside the pure-content key.
#[inline]
pub(crate) fn has_macro_type_deps(input: &CompileInput) -> bool {
    !input.macro_type_deps.is_empty()
}

/// True iff any of the compile input's script imports resolves
/// through a workspace alias.
///
/// Workspace aliases are matched by prefix: a specifier of the form
/// `<alias.find><rest>` is considered an alias hit. The host hands
/// the classifier the alias list at call time so the classifier can
/// stay pure (no `&dyn ResolverContext` heavy lift). When no aliases
/// are configured the predicate returns `false` cheaply.
#[inline]
pub(crate) fn has_workspace_alias(input: &CompileInput, aliases: &[WorkspaceAlias]) -> bool {
    if aliases.is_empty() {
        return false;
    }
    for import in &input.script_imports {
        if specifier_matches_any_alias(&import.source, aliases) {
            return true;
        }
    }
    false
}

/// Per-specifier alias-prefix match. `find` is matched as a prefix —
/// the workspace's own resolver applies the same prefix-replacement,
/// so a specifier triggers an alias hit when its raw text begins
/// with the alias's `find`.
fn specifier_matches_any_alias(specifier: &str, aliases: &[WorkspaceAlias]) -> bool {
    for alias in aliases {
        if alias.find.is_empty() {
            continue;
        }
        if specifier == alias.find || specifier.starts_with(&alias.find) {
            return true;
        }
    }
    false
}

/// True iff `canonical_id` or any of its directly imported canonicals
/// participates in module augmentation under the current project's
/// resolve / lib env.
///
/// Routes through
/// [`crate::file_artifact_store::FileArtifactStore`]: a single
/// matching augmentation entry in the per-canonical
/// `FileArtifacts.augmentations` is sufficient. The classifier checks
/// the owner canonical's artifacts (cheap) — directly imported files
/// are NOT walked here; the cold-build path's fact tracer captures
/// downstream augmentation observations under `Session` mode.
///
/// The host pre-computes the boolean and hands it to
/// [`EligibilityInputs::owner_has_module_augmentation`] so the
/// classifier itself stays free of `&FileArtifactStore`. Tests build
/// `EligibilityInputs` directly without an artifact store.
#[inline]
pub(crate) fn has_module_augmentation(
    canonical_id: &str,
    store: &crate::file_artifact_store::FileArtifactStore,
) -> bool {
    if let Some(artifacts) = store.get_artifacts_any(canonical_id) {
        if !artifacts.augmentations.is_empty() {
            return true;
        }
    }
    false
}

/// True iff the compile input carries a block override (preprocessed
/// script / template).
#[inline]
pub(crate) fn has_block_override(input: &CompileInput) -> bool {
    input.content_override_layer.is_some()
}

/// True iff the compile input carries a style override (preprocessed
/// CSS).
#[inline]
pub(crate) fn has_style_override(input: &CompileInput) -> bool {
    input.style_override_layer.is_some()
}

/// True iff the compile profile target is IDE-only analysis
/// (`CompileTarget::TSX` set without `CompileTarget::TEMPLATE`). IDE-
/// only analysis publishes through a different cache shape (the TSX
/// output lives on the same compile slot as the bundler outputs, but
/// the consumer is the LSP / TypeProvider and not the bundler).
#[inline]
pub(crate) fn has_ide_only_analysis(profile: &CompileProfile) -> bool {
    use verter_compiler::compile::CompileTarget;
    profile.target.contains(CompileTarget::TSX) && !profile.target.contains(CompileTarget::TEMPLATE)
}

/// True iff the host is in dev mode with
/// [`CompileErrorPolicy::DevServeLastKnownGood`]. The dev-last-good
/// fallback requires per-session slot state.
#[inline]
pub(crate) fn has_dev_last_good(config: &HostConfig) -> bool {
    config.dev_mode && config.compile_error_policy == CompileErrorPolicy::DevServeLastKnownGood
}

/// Inputs the classifier reads from the host. The classifier itself
/// stays pure — the caller assembles this view at call time.
///
/// Decomposing the inputs keeps the classifier testable: a test
/// constructs an `EligibilityInputs` directly with the relevant
/// flags / lists, no host required.
///
/// `pub(crate)` because [`CompileInput`] is `pub(crate)` and the
/// classifier is consumed only by the in-crate compile entry path.
pub(crate) struct EligibilityInputs<'a> {
    /// The compile input the cold-build sees.
    pub(crate) input: &'a CompileInput,
    /// The compile profile (target flags, source-map policy, etc.).
    pub(crate) profile: &'a CompileProfile,
    /// Host configuration — needed for dev / compile-error-policy
    /// gating.
    pub(crate) config: &'a HostConfig,
    /// Configured workspace aliases for the owning project. Empty
    /// when no aliases are configured.
    pub(crate) workspace_aliases: &'a [WorkspaceAlias],
    /// Whether the compile input's owning canonical participates in
    /// module augmentation under the live project resolve / lib env.
    ///
    /// The host pre-computes this via [`has_module_augmentation`] (a
    /// `FileArtifactStore`-backed check) and hands the resulting bool
    /// to the classifier so the classifier itself stays free of
    /// `&dyn ResolverContext`. Tests can pass `true` / `false`
    /// directly without an artifact store.
    pub(crate) owner_has_module_augmentation: bool,
}

/// Classify a requested compile-cache mode against the request's
/// eligibility surface.
///
/// Walks the eight downgrade predicates in priority order, collects
/// every triggering reason into
/// [`CompileModeClassification::downgrade_reasons`], and selects the
/// `actual_mode` per the downgrade ladder:
///
/// * [`CompileCacheMode::Session`] downgrades on ANY reason →
///   `Content` first, then `Stateless` if `Content` is also not
///   eligible.
/// * [`CompileCacheMode::Content`] downgrades on ANY reason →
///   `Stateless`.
/// * [`CompileCacheMode::Stateless`] never downgrades — it already
///   bypasses host caches.
///
/// Stateless mode IGNORES every reason: the `downgrade_reasons`
/// vector is empty even when conditions like `HasMacroTypeDeps` are
/// triggered, because stateless ALREADY bypasses every cache the
/// downgrade reasons would protect.
pub(crate) fn classify_compile_mode(
    requested: CompileCacheMode,
    inputs: &EligibilityInputs<'_>,
) -> CompileModeClassification {
    // Stateless floor — ignore reasons entirely.
    if requested == CompileCacheMode::Stateless {
        return CompileModeClassification {
            requested_mode: requested,
            actual_mode: CompileCacheMode::Stateless,
            downgrade_reasons: SmallVec::new(),
        };
    }

    let mut reasons: SmallVec<[DowngradeReason; 2]> = SmallVec::new();

    // Priority order — same ordering for both `Content` and
    // `Session`, since each predicate is a single boolean and the
    // mode-downgrade fold below decides where the request lands.
    if inputs.owner_has_module_augmentation {
        reasons.push(DowngradeReason::HasModuleAugmentation);
    }
    if has_macro_type_deps(inputs.input) {
        reasons.push(DowngradeReason::HasMacroTypeDeps);
    }
    if has_workspace_alias(inputs.input, inputs.workspace_aliases) {
        reasons.push(DowngradeReason::HasWorkspaceAlias);
    }
    if has_external_src(inputs.input) {
        reasons.push(DowngradeReason::HasExternalSrc);
    }
    if has_block_override(inputs.input) {
        reasons.push(DowngradeReason::HasBlockOverride);
    }
    if has_style_override(inputs.input) {
        reasons.push(DowngradeReason::HasStyleOverride);
    }
    if has_ide_only_analysis(inputs.profile) {
        reasons.push(DowngradeReason::HasIdeOnlyAnalysis);
    }
    if has_dev_last_good(inputs.config) {
        reasons.push(DowngradeReason::HasDevLastGood);
    }

    if reasons.is_empty() {
        return CompileModeClassification {
            requested_mode: requested,
            actual_mode: requested,
            downgrade_reasons: SmallVec::new(),
        };
    }

    // Mode downgrade fold. Every reason in the current predicate set
    // invalidates BOTH the `Content` and `Session` cache shapes —
    // they share the same content-key surface, so any observed reason
    // collapses the request to `Stateless`. If an eligibility
    // refinement later isolates reasons applicable only to `Session`
    // (leaving `Content` still admissible), this arm splits per-mode;
    // under the present predicates the collapse is the correct floor.
    let actual_mode = match requested {
        CompileCacheMode::Session => CompileCacheMode::Stateless,
        CompileCacheMode::Content => CompileCacheMode::Stateless,
        CompileCacheMode::Stateless => CompileCacheMode::Stateless,
    };

    CompileModeClassification {
        requested_mode: requested,
        actual_mode,
        downgrade_reasons: reasons,
    }
}

#[cfg(test)]
#[path = "compile_cache_mode_tests.rs"]
mod tests;
