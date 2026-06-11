//! Unit tests for the compile-cache mode classifier.
//!
//! Two coverage axes:
//!
//! 1. **Per-helper tests** — each eligibility predicate
//!    ([`has_external_src`], [`has_macro_type_deps`],
//!    [`has_workspace_alias`], [`has_block_override`],
//!    [`has_style_override`], [`has_ide_only_analysis`],
//!    [`has_dev_last_good`]) has a positive + negative case. The
//!    module-augmentation reason is covered indirectly by the
//!    classifier-level table (the host computes the boolean via
//!    `VerterHost::owner_has_module_augmentation_dependency` and hands
//!    it to `EligibilityInputs.owner_has_module_augmentation`).
//!
//! 2. **Table-driven classifier test** — exercises every variant of
//!    [`CompileCacheMode`] against every priority-ordered reason
//!    interaction. The ordering oracle is tested explicitly: an
//!    all-reasons row pins the exact priority list.
//!
//! Mode-fold contract under test:
//!
//! * `Session` stays `Session` for ANY reason (the session fact rail /
//!   per-session slot state handles every reason); the reasons are
//!   still recorded in `downgrade_reasons` for telemetry.
//! * `Content` downgrades to `Stateless` on ANY reason (the pure
//!   content key cannot represent a cross-file / session-scoped /
//!   IDE-shape input); there is no `Content → Session` promotion.
//! * `Stateless` is the floor and ignores every reason (empty
//!   `downgrade_reasons`).
//!
//! Stub Prevention guarantee: every classifier test asserts a
//! discriminating predicate. Each `Session + reason → Session` case
//! asserts BOTH `actual_mode == Session` AND the exact ordered
//! `downgrade_reasons` slice, so it FAILS against any fold that
//! collapses `Session` to `Stateless` and never degenerates into a
//! tautology.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use smallvec::smallvec;
use verter_compiler::compile::CompileTarget;
use verter_semantic::analysis::{
    AnalyzedImport, AnalyzedImportBinding, AnalyzedMacroKind, MacroTypeDep,
};
use verter_semantic::input::ImportBindingKind;
use verter_span::Span;
use verter_workspace::WorkspaceAlias;

use super::*;
use crate::types::{
    CompileErrorPolicy, CompileInput, CompileProfile, ContentOverrideLayer, DiagnosticsSnapshot,
    ExternalBlockKind, ExternalSourceRequest, FileMeta, HostConfig, StyleOverrideLayer,
};

// ── Fixture builders ─────────────────────────────────────────────────

fn empty_file_meta() -> FileMeta {
    FileMeta {
        has_script: false,
        has_template: false,
        has_scoped_style: false,
        script_lang: None,
        template_lang: None,
        style_langs: Vec::new(),
        custom_types: Vec::new(),
        custom_langs: Vec::new(),
    }
}

fn empty_input() -> CompileInput {
    CompileInput {
        canonical_id: "/a.vue".to_string(),
        source: Arc::<str>::from(""),
        meta: empty_file_meta(),
        parse_diagnostics: DiagnosticsSnapshot::default(),
        src_blocks: Vec::new(),
        external_requests: Vec::new(),
        style_override_layer: None,
        content_override_layer: None,
        macro_type_deps: Vec::new(),
        script_imports: Vec::new(),
        script_macros: Vec::new(),
        script_bindings: Vec::new(),
        framework_parse: None,
        style_v_bind_vars: Vec::new(),
    }
}

fn make_external_src_request() -> ExternalSourceRequest {
    // Smallest valid request — fields are only consulted by the
    // compile entry path; the classifier only checks emptiness.
    ExternalSourceRequest {
        owner_canonical_id: "/a.vue".to_string(),
        block_kind: ExternalBlockKind::Script,
        index: 0,
        specifier: "./external.ts".to_string(),
        resolved_canonical_id: "/external.ts".to_string(),
    }
}

fn make_macro_type_dep(name: &str) -> MacroTypeDep {
    MacroTypeDep {
        type_name: name.to_string(),
        import_source: "./types".to_string(),
        macro_kind: AnalyzedMacroKind::DefineProps,
        macro_index: 0,
        macro_span: Span::new(0, 0),
    }
}

fn make_alias_import(spec: &str) -> AnalyzedImport {
    AnalyzedImport {
        source: spec.to_string(),
        is_type_only: false,
        bindings: vec![AnalyzedImportBinding {
            name: "X".to_string(),
            kind: ImportBindingKind::Named,
            imported_name: Some("X".to_string()),
            is_type_only: false,
            vue_api: None,
            span: Span::new(0, 0),
        }],
        span: Span::new(0, 0),
        resolved_canonical_id: None,
    }
}

fn make_style_override_layer() -> StyleOverrideLayer {
    StyleOverrideLayer {
        hash: 1,
        by_index: FxHashMap::default(),
    }
}

fn make_content_override_layer() -> ContentOverrideLayer {
    ContentOverrideLayer {
        hash: 1,
        template: None,
        script: None,
    }
}

fn default_profile() -> CompileProfile {
    CompileProfile::default() // target = BUNDLER (no TSX bit, no IDE-only).
}

fn ide_only_profile() -> CompileProfile {
    // TSX without TEMPLATE → IDE-only.
    CompileProfile {
        target: CompileTarget::TSX,
        ..CompileProfile::default()
    }
}

fn non_dev_last_good_config() -> HostConfig {
    HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    }
}

fn dev_last_good_config() -> HostConfig {
    HostConfig {
        dev_mode: true,
        compile_error_policy: CompileErrorPolicy::DevServeLastKnownGood,
        ..HostConfig::default()
    }
}

fn alias(find: &str, replacement: &str) -> WorkspaceAlias {
    WorkspaceAlias {
        find: find.to_string(),
        replacement: replacement.to_string(),
    }
}

/// Build an EligibilityInputs from the provided components.
struct InputsBundle {
    input: CompileInput,
    profile: CompileProfile,
    config: HostConfig,
    aliases: Vec<WorkspaceAlias>,
    owner_aug: bool,
}

impl InputsBundle {
    fn empty() -> Self {
        Self {
            input: empty_input(),
            profile: default_profile(),
            config: non_dev_last_good_config(),
            aliases: Vec::new(),
            owner_aug: false,
        }
    }

    fn view(&self) -> EligibilityInputs<'_> {
        EligibilityInputs {
            input: &self.input,
            profile: &self.profile,
            config: &self.config,
            workspace_aliases: &self.aliases,
            owner_has_module_augmentation: self.owner_aug,
        }
    }
}

// ── Per-helper tests ─────────────────────────────────────────────────

#[test]
fn has_external_src_positive_and_negative() {
    let mut input = empty_input();
    assert!(!has_external_src(&input));
    input.external_requests.push(make_external_src_request());
    assert!(has_external_src(&input));
}

#[test]
fn has_macro_type_deps_positive_and_negative() {
    let mut input = empty_input();
    assert!(!has_macro_type_deps(&input));
    input
        .macro_type_deps
        .push(make_macro_type_dep("BadgeProps"));
    assert!(has_macro_type_deps(&input));
}

#[test]
fn has_workspace_alias_empty_alias_list_is_false() {
    let mut input = empty_input();
    input.script_imports.push(make_alias_import("@/anything"));
    // No aliases configured → predicate is `false` cheaply.
    assert!(!has_workspace_alias(&input, &[]));
}

#[test]
fn has_workspace_alias_no_imports_is_false() {
    let input = empty_input();
    let aliases = vec![alias("@/", "/workspace/src/")];
    // No script imports → no alias hits.
    assert!(!has_workspace_alias(&input, &aliases));
}

#[test]
fn has_workspace_alias_prefix_match_positive() {
    let mut input = empty_input();
    input
        .script_imports
        .push(make_alias_import("@/components/Foo"));
    let aliases = vec![alias("@/", "/workspace/src/")];
    assert!(has_workspace_alias(&input, &aliases));
}

#[test]
fn has_workspace_alias_exact_match_positive() {
    let mut input = empty_input();
    input.script_imports.push(make_alias_import("@"));
    let aliases = vec![alias("@", "/workspace/src/")];
    assert!(has_workspace_alias(&input, &aliases));
}

#[test]
fn has_workspace_alias_non_matching_specifier_negative() {
    let mut input = empty_input();
    input.script_imports.push(make_alias_import("vue"));
    let aliases = vec![alias("@/", "/workspace/src/")];
    assert!(!has_workspace_alias(&input, &aliases));
}

#[test]
fn has_workspace_alias_empty_find_is_skipped() {
    // An empty `find` string would prefix-match every specifier — the
    // helper explicitly skips empty-find entries.
    let mut input = empty_input();
    input.script_imports.push(make_alias_import("./relative"));
    let aliases = vec![alias("", "/anywhere/")];
    assert!(!has_workspace_alias(&input, &aliases));
}

#[test]
fn has_block_override_positive_and_negative() {
    let mut input = empty_input();
    assert!(!has_block_override(&input));
    input.content_override_layer = Some(make_content_override_layer());
    assert!(has_block_override(&input));
}

#[test]
fn has_style_override_positive_and_negative() {
    let mut input = empty_input();
    assert!(!has_style_override(&input));
    input.style_override_layer = Some(make_style_override_layer());
    assert!(has_style_override(&input));
}

#[test]
fn has_ide_only_analysis_positive_and_negative() {
    // TSX without TEMPLATE → IDE-only.
    let ide = ide_only_profile();
    assert!(has_ide_only_analysis(&ide));
    // BUNDLER (no TSX bit) → not IDE-only.
    let bundler = default_profile();
    assert!(!has_ide_only_analysis(&bundler));
    // TSX | TEMPLATE → not IDE-only (combined target).
    let combined = CompileProfile {
        target: CompileTarget::TSX | CompileTarget::TEMPLATE,
        ..CompileProfile::default()
    };
    assert!(!has_ide_only_analysis(&combined));
}

#[test]
fn has_dev_last_good_positive_and_negative() {
    // Both dev + policy required.
    let dev_good = dev_last_good_config();
    assert!(has_dev_last_good(&dev_good));

    let prod = non_dev_last_good_config();
    assert!(!has_dev_last_good(&prod));

    // Dev only, strict policy.
    let dev_strict = HostConfig {
        dev_mode: true,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    };
    assert!(!has_dev_last_good(&dev_strict));

    // Last-good policy without dev.
    let prod_last_good = HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::DevServeLastKnownGood,
        ..HostConfig::default()
    };
    assert!(!has_dev_last_good(&prod_last_good));
}

// ── first_downgrade_reason ───────────────────────────────────────────

#[test]
fn first_downgrade_reason_returns_first_when_present() {
    let cls = CompileModeClassification {
        requested_mode: CompileCacheMode::Content,
        actual_mode: CompileCacheMode::Stateless,
        downgrade_reasons: smallvec![
            DowngradeReason::HasMacroTypeDeps,
            DowngradeReason::HasExternalSrc,
        ],
    };
    assert_eq!(
        cls.first_downgrade_reason(),
        Some(DowngradeReason::HasMacroTypeDeps)
    );
}

#[test]
fn first_downgrade_reason_returns_none_when_empty() {
    let cls = CompileModeClassification {
        requested_mode: CompileCacheMode::Session,
        actual_mode: CompileCacheMode::Session,
        downgrade_reasons: SmallVec::new(),
    };
    assert!(cls.first_downgrade_reason().is_none());
}

// ── Classifier: trivial happy paths ──────────────────────────────────

#[test]
fn classifier_session_no_reasons_stays_session() {
    let bundle = InputsBundle::empty();
    let cls = classify_compile_mode(CompileCacheMode::Session, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Session);
    assert!(cls.downgrade_reasons.is_empty());
    assert_eq!(cls.requested_mode, CompileCacheMode::Session);
}

#[test]
fn classifier_content_no_reasons_stays_content() {
    let bundle = InputsBundle::empty();
    let cls = classify_compile_mode(CompileCacheMode::Content, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Content);
    assert!(cls.downgrade_reasons.is_empty());
}

#[test]
fn classifier_stateless_no_reasons_stays_stateless() {
    let bundle = InputsBundle::empty();
    let cls = classify_compile_mode(CompileCacheMode::Stateless, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Stateless);
    assert!(cls.downgrade_reasons.is_empty());
}

// ── Classifier: single reason — Session stays Session ────────────────
//
// Each test asserts BOTH `actual_mode == Session` (FAILS against a fold
// that collapses Session to Stateless) AND the exact ordered reason
// slice (so the assertion never degenerates into a tautology).

#[test]
fn classifier_session_with_module_augmentation_stays_session() {
    let mut bundle = InputsBundle::empty();
    bundle.owner_aug = true;
    let cls = classify_compile_mode(CompileCacheMode::Session, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Session);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[DowngradeReason::HasModuleAugmentation]
    );
}

#[test]
fn classifier_session_with_workspace_alias_stays_session() {
    let mut bundle = InputsBundle::empty();
    bundle.input.script_imports.push(make_alias_import("@/foo"));
    bundle.aliases.push(alias("@/", "/src/"));
    let cls = classify_compile_mode(CompileCacheMode::Session, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Session);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[DowngradeReason::HasWorkspaceAlias]
    );
}

#[test]
fn classifier_session_with_external_src_stays_session() {
    let mut bundle = InputsBundle::empty();
    bundle
        .input
        .external_requests
        .push(make_external_src_request());
    let cls = classify_compile_mode(CompileCacheMode::Session, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Session);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[DowngradeReason::HasExternalSrc]
    );
}

#[test]
fn classifier_session_with_block_override_stays_session() {
    let mut bundle = InputsBundle::empty();
    bundle.input.content_override_layer = Some(make_content_override_layer());
    let cls = classify_compile_mode(CompileCacheMode::Session, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Session);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[DowngradeReason::HasBlockOverride]
    );
}

#[test]
fn classifier_session_with_style_override_stays_session() {
    let mut bundle = InputsBundle::empty();
    bundle.input.style_override_layer = Some(make_style_override_layer());
    let cls = classify_compile_mode(CompileCacheMode::Session, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Session);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[DowngradeReason::HasStyleOverride]
    );
}

#[test]
fn classifier_session_with_ide_only_analysis_stays_session() {
    let mut bundle = InputsBundle::empty();
    bundle.profile = ide_only_profile();
    let cls = classify_compile_mode(CompileCacheMode::Session, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Session);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[DowngradeReason::HasIdeOnlyAnalysis]
    );
}

#[test]
fn classifier_session_with_dev_last_good_stays_session() {
    let mut bundle = InputsBundle::empty();
    bundle.config = dev_last_good_config();
    let cls = classify_compile_mode(CompileCacheMode::Session, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Session);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[DowngradeReason::HasDevLastGood]
    );
}

// ── Classifier: single reason — Content downgrades to Stateless ──────
//
// Every reason makes the pure content key unsafe, so an explicit
// Content request floors to Stateless. Each test asserts the floor AND
// preserves the recorded reason slice.

#[test]
fn classifier_content_with_module_augmentation_downgrades_to_stateless() {
    let mut bundle = InputsBundle::empty();
    bundle.owner_aug = true;
    let cls = classify_compile_mode(CompileCacheMode::Content, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Stateless);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[DowngradeReason::HasModuleAugmentation]
    );
}

#[test]
fn classifier_content_with_macro_type_deps_downgrades_to_stateless() {
    let mut bundle = InputsBundle::empty();
    bundle.input.macro_type_deps.push(make_macro_type_dep("X"));
    let cls = classify_compile_mode(CompileCacheMode::Content, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Stateless);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[DowngradeReason::HasMacroTypeDeps]
    );
}

#[test]
fn classifier_content_with_workspace_alias_downgrades_to_stateless() {
    let mut bundle = InputsBundle::empty();
    bundle.input.script_imports.push(make_alias_import("@/foo"));
    bundle.aliases.push(alias("@/", "/src/"));
    let cls = classify_compile_mode(CompileCacheMode::Content, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Stateless);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[DowngradeReason::HasWorkspaceAlias]
    );
}

#[test]
fn classifier_content_with_external_src_downgrades_to_stateless() {
    let mut bundle = InputsBundle::empty();
    bundle
        .input
        .external_requests
        .push(make_external_src_request());
    let cls = classify_compile_mode(CompileCacheMode::Content, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Stateless);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[DowngradeReason::HasExternalSrc]
    );
}

#[test]
fn classifier_content_with_block_override_downgrades_to_stateless() {
    let mut bundle = InputsBundle::empty();
    bundle.input.content_override_layer = Some(make_content_override_layer());
    let cls = classify_compile_mode(CompileCacheMode::Content, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Stateless);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[DowngradeReason::HasBlockOverride]
    );
}

#[test]
fn classifier_content_with_style_override_downgrades_to_stateless() {
    let mut bundle = InputsBundle::empty();
    bundle.input.style_override_layer = Some(make_style_override_layer());
    let cls = classify_compile_mode(CompileCacheMode::Content, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Stateless);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[DowngradeReason::HasStyleOverride]
    );
}

#[test]
fn classifier_content_with_ide_only_analysis_downgrades_to_stateless() {
    let mut bundle = InputsBundle::empty();
    bundle.profile = ide_only_profile();
    let cls = classify_compile_mode(CompileCacheMode::Content, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Stateless);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[DowngradeReason::HasIdeOnlyAnalysis]
    );
}

#[test]
fn classifier_content_with_dev_last_good_downgrades_to_stateless() {
    let mut bundle = InputsBundle::empty();
    bundle.config = dev_last_good_config();
    let cls = classify_compile_mode(CompileCacheMode::Content, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Stateless);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[DowngradeReason::HasDevLastGood]
    );
}

// ── Classifier: priority ordering (Session stays Session) ────────────
//
// The reason ordering is identical regardless of requested mode; these
// drive a Session request and assert Session + the ordered slice.

#[test]
fn classifier_two_reasons_emit_in_priority_order() {
    // IdeOnlyAnalysis + DevLastGood — IdeOnlyAnalysis is higher
    // priority than DevLastGood.
    let mut bundle = InputsBundle::empty();
    bundle.profile = ide_only_profile();
    bundle.config = dev_last_good_config();
    let cls = classify_compile_mode(CompileCacheMode::Session, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Session);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[
            DowngradeReason::HasIdeOnlyAnalysis,
            DowngradeReason::HasDevLastGood,
        ]
    );
    assert_eq!(
        cls.first_downgrade_reason(),
        Some(DowngradeReason::HasIdeOnlyAnalysis)
    );
}

#[test]
fn classifier_mid_priority_mix_macro_then_alias_then_external() {
    let mut bundle = InputsBundle::empty();
    bundle.input.macro_type_deps.push(make_macro_type_dep("X"));
    bundle.input.script_imports.push(make_alias_import("@/foo"));
    bundle.aliases.push(alias("@/", "/src/"));
    bundle
        .input
        .external_requests
        .push(make_external_src_request());
    let cls = classify_compile_mode(CompileCacheMode::Session, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Session);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[
            DowngradeReason::HasMacroTypeDeps,
            DowngradeReason::HasWorkspaceAlias,
            DowngradeReason::HasExternalSrc,
        ]
    );
}

#[test]
fn classifier_block_then_style_override_then_ide() {
    let mut bundle = InputsBundle::empty();
    bundle.input.content_override_layer = Some(make_content_override_layer());
    bundle.input.style_override_layer = Some(make_style_override_layer());
    bundle.profile = ide_only_profile();
    let cls = classify_compile_mode(CompileCacheMode::Session, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Session);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[
            DowngradeReason::HasBlockOverride,
            DowngradeReason::HasStyleOverride,
            DowngradeReason::HasIdeOnlyAnalysis,
        ]
    );
}

#[test]
fn classifier_module_augmentation_beats_macro_type_deps() {
    let mut bundle = InputsBundle::empty();
    bundle.owner_aug = true;
    bundle.input.macro_type_deps.push(make_macro_type_dep("X"));
    let cls = classify_compile_mode(CompileCacheMode::Session, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Session);
    assert_eq!(
        cls.first_downgrade_reason(),
        Some(DowngradeReason::HasModuleAugmentation)
    );
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[
            DowngradeReason::HasModuleAugmentation,
            DowngradeReason::HasMacroTypeDeps,
        ]
    );
}

// ── Classifier: full ordering with all eight reasons triggered ───────

#[test]
fn classifier_all_reasons_triggered_emit_full_priority_list() {
    let mut bundle = InputsBundle::empty();
    bundle.owner_aug = true;
    bundle.input.macro_type_deps.push(make_macro_type_dep("X"));
    bundle.input.script_imports.push(make_alias_import("@/foo"));
    bundle.aliases.push(alias("@/", "/src/"));
    bundle
        .input
        .external_requests
        .push(make_external_src_request());
    bundle.input.content_override_layer = Some(make_content_override_layer());
    bundle.input.style_override_layer = Some(make_style_override_layer());
    bundle.profile = ide_only_profile();
    bundle.config = dev_last_good_config();

    let cls = classify_compile_mode(CompileCacheMode::Session, &bundle.view());
    // Session stays Session even with every reason firing.
    assert_eq!(cls.actual_mode, CompileCacheMode::Session);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[
            DowngradeReason::HasModuleAugmentation,
            DowngradeReason::HasMacroTypeDeps,
            DowngradeReason::HasWorkspaceAlias,
            DowngradeReason::HasExternalSrc,
            DowngradeReason::HasBlockOverride,
            DowngradeReason::HasStyleOverride,
            DowngradeReason::HasIdeOnlyAnalysis,
            DowngradeReason::HasDevLastGood,
        ]
    );
    assert_eq!(
        cls.first_downgrade_reason(),
        Some(DowngradeReason::HasModuleAugmentation)
    );
}

#[test]
fn classifier_content_with_all_reasons_floors_to_stateless_with_full_list() {
    // Same all-reasons input as above, but an explicit Content request:
    // floors to Stateless while preserving the full ordered reason list.
    let mut bundle = InputsBundle::empty();
    bundle.owner_aug = true;
    bundle.input.macro_type_deps.push(make_macro_type_dep("X"));
    bundle.input.script_imports.push(make_alias_import("@/foo"));
    bundle.aliases.push(alias("@/", "/src/"));
    bundle
        .input
        .external_requests
        .push(make_external_src_request());
    bundle.input.content_override_layer = Some(make_content_override_layer());
    bundle.input.style_override_layer = Some(make_style_override_layer());
    bundle.profile = ide_only_profile();
    bundle.config = dev_last_good_config();

    let cls = classify_compile_mode(CompileCacheMode::Content, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Stateless);
    assert_eq!(cls.requested_mode, CompileCacheMode::Content);
    assert_eq!(
        cls.downgrade_reasons.as_slice(),
        &[
            DowngradeReason::HasModuleAugmentation,
            DowngradeReason::HasMacroTypeDeps,
            DowngradeReason::HasWorkspaceAlias,
            DowngradeReason::HasExternalSrc,
            DowngradeReason::HasBlockOverride,
            DowngradeReason::HasStyleOverride,
            DowngradeReason::HasIdeOnlyAnalysis,
            DowngradeReason::HasDevLastGood,
        ]
    );
}

// ── Classifier: Stateless floor IGNORES every reason ─────────────────

#[test]
fn classifier_stateless_with_all_reasons_keeps_reasons_empty() {
    // Stateless requested + every reason triggered → actual_mode
    // stays Stateless, downgrade_reasons MUST be empty (stateless
    // ALREADY bypasses every cache the reasons protect).
    let mut bundle = InputsBundle::empty();
    bundle.owner_aug = true;
    bundle.input.macro_type_deps.push(make_macro_type_dep("X"));
    bundle.input.script_imports.push(make_alias_import("@/foo"));
    bundle.aliases.push(alias("@/", "/src/"));
    bundle
        .input
        .external_requests
        .push(make_external_src_request());
    bundle.input.content_override_layer = Some(make_content_override_layer());
    bundle.input.style_override_layer = Some(make_style_override_layer());
    bundle.profile = ide_only_profile();
    bundle.config = dev_last_good_config();

    let cls = classify_compile_mode(CompileCacheMode::Stateless, &bundle.view());
    assert_eq!(cls.actual_mode, CompileCacheMode::Stateless);
    assert_eq!(cls.requested_mode, CompileCacheMode::Stateless);
    assert!(cls.downgrade_reasons.is_empty());
    assert!(cls.first_downgrade_reason().is_none());
}

// ── Smoke: classifier preserves requested_mode on a Content floor ────

#[test]
fn classifier_preserves_requested_mode_on_content_downgrade() {
    // A Content request with a reason floors actual_mode to Stateless
    // while preserving requested_mode == Content. (Session + reason is
    // no longer a downgrade under the corrected fold, so the
    // requested-mode-preservation smoke uses a Content request.)
    let mut bundle = InputsBundle::empty();
    bundle.owner_aug = true;
    let cls = classify_compile_mode(CompileCacheMode::Content, &bundle.view());
    assert_eq!(cls.requested_mode, CompileCacheMode::Content);
    assert_eq!(cls.actual_mode, CompileCacheMode::Stateless);
    assert_ne!(cls.actual_mode, cls.requested_mode);
}
