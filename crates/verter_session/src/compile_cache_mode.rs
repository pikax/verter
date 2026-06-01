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
//! Most downgrade reasons have a small, testable eligibility predicate
//! ([`has_external_src`], [`has_macro_type_deps`],
//! [`has_workspace_alias`], [`has_block_override`],
//! [`has_style_override`], [`has_ide_only_analysis`],
//! [`has_dev_last_good`]). The module-augmentation reason is the one
//! exception: it requires the augmentation TARGET index for every
//! module the owner can consume, so the host computes it
//! (`VerterHost::owner_has_module_augmentation_dependency`) and hands
//! the resulting boolean to
//! [`EligibilityInputs::owner_has_module_augmentation`], keeping the
//! classifier itself free of `&FileArtifactStore`. The composite
//! [`classify_compile_mode`] walks every reason in deterministic
//! priority order, collects each triggering reason into the
//! [`CompileModeClassification::downgrade_reasons`] vector, and folds
//! the requested mode to the mode the inputs actually support.
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
//! ## Mode fold
//!
//! Every reason is either a cross-file dependency
//! (`HasMacroTypeDeps` / `HasModuleAugmentation` / `HasWorkspaceAlias`
//! / `HasExternalSrc`) or a session-scoped / IDE-shape input
//! (`HasBlockOverride` / `HasStyleOverride` / `HasIdeOnlyAnalysis` /
//! `HasDevLastGood`). The session cache's path-precise
//! [`ReadSetSignature`](crate::fact_signature_helpers::ReadSetSignature)
//! fact rail and per-session slot state handle all of them, so:
//!
//! * [`CompileCacheMode::Session`] stays `Session` for EVERY reason —
//!   the reasons are recorded in `downgrade_reasons` for telemetry but
//!   the mode does not change. `Session` is the host default and the
//!   richest mode.
//! * [`CompileCacheMode::Content`] is a pure content-addressed request
//!   with NO fact rail; ANY reason means its pure key cannot represent
//!   the input safely, so `Content` downgrades to
//!   [`CompileCacheMode::Stateless`]. There is NO `Content → Session`
//!   promotion: `Content` is an explicit opt-in whose
//!   safety-precondition failure floors to `Stateless`.
//! * [`CompileCacheMode::Stateless`] never changes — it is the floor
//!   and ignores every reason (stateless already bypasses host caches).

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
    /// All triggering eligibility reasons in priority order, retained for
    /// telemetry whenever any predicate fires — even when the mode does
    /// NOT change. A `Session` request stays `Session` under every reason,
    /// so this vector is frequently non-empty while
    /// `actual_mode == requested_mode`. The vector is empty only in two
    /// cases: the `Stateless` floor (which ignores all reasons) and the
    /// no-reason case (no predicate fired).
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
    /// Whether a compile of the owning canonical could consume any
    /// module augmentation reachable from its declaration graph, under
    /// the live project resolve / lib env.
    ///
    /// The host pre-computes this via
    /// `VerterHost::owner_has_module_augmentation_dependency`, which
    /// consults the augmentation TARGET index for every module the owner
    /// can consume (its imported specifiers) plus ambient / global
    /// augmenters — so an imported / ambient augmenter that leaves no
    /// trace on the owner's own `FileArtifacts.augmentations` is still
    /// caught. The classifier itself stays free of `&FileArtifactStore`;
    /// tests pass `true` / `false` directly without an artifact store.
    pub(crate) owner_has_module_augmentation: bool,
}

/// Classify a requested compile-cache mode against the request's
/// eligibility surface.
///
/// Walks the eight predicates in priority order, collects every
/// triggering reason into
/// [`CompileModeClassification::downgrade_reasons`], and selects the
/// `actual_mode` per the mode fold:
///
/// * [`CompileCacheMode::Session`] stays `Session` for ANY reason —
///   the session fact rail / per-session slot state handles every
///   reason, so the mode is unchanged and the reasons are recorded
///   only for telemetry.
/// * [`CompileCacheMode::Content`] downgrades to `Stateless` on ANY
///   reason — the pure content key cannot represent a cross-file /
///   session-scoped / IDE-shape input. There is no `Content → Session`
///   promotion.
/// * [`CompileCacheMode::Stateless`] never changes — it already
///   bypasses host caches.
///
/// Stateless mode IGNORES every reason: the `downgrade_reasons`
/// vector is empty even when conditions like `HasMacroTypeDeps` are
/// triggered, because stateless ALREADY bypasses every cache the
/// reasons would protect.
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

    // Mode fold. Every reason is a cross-file / session-scoped /
    // IDE-shape input that the session fact rail (or per-session slot
    // state) handles safely, so `Session` stays `Session` for every
    // reason — the reasons remain in `downgrade_reasons` for telemetry
    // even though the mode does not change. `Content` is a pure
    // content-addressed request with NO fact rail: any reason means the
    // pure key cannot represent the input safely, so `Content`
    // downgrades to `Stateless`. There is NO `Content → Session`
    // promotion — `Content` is an explicit opt-in and its
    // safety-precondition failure floors to `Stateless`. `Stateless`
    // already returned above (it is the floor and ignores reasons).
    let actual_mode = match requested {
        CompileCacheMode::Stateless => CompileCacheMode::Stateless,
        CompileCacheMode::Session => CompileCacheMode::Session,
        CompileCacheMode::Content => CompileCacheMode::Stateless,
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
