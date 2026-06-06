//! R21 isolation tests — each of the five env-hash dimensions isolates
//! the correct cache layer.
//!
//! The unit tests in `crates/verter_workspace/src/env_hash_tests.rs` cover
//! the pure hash discrimination (each dimension's hash changes when its own
//! inputs change, and the other 4 stay constant). This integration test
//! exercises the **scoping rule** angle: when a `lib_env_hash` input
//! changes, the consumer caches that DO depend on lib data MUST observe a
//! different key; the consumer caches that do NOT depend on lib data
//! (`ResolvedImportFacts` per R21 scoping rule, `FileArtifactStore` because
//! it is parse-domain-only) MUST observe an UNCHANGED key.
//!
//! `FileArtifactStore.augmentation_index` keys (`AugmentationTargetKey`)
//! consume `lib_env_hash`, so the test contrasts:
//!
//! 1. `FileArtifactKey` (the artifact key) — does NOT carry `lib_env_hash`;
//!    a lib edit MUST NOT change it.
//! 2. `AugmentationTargetKey` (the augmentation-index key) — DOES carry
//!    `lib_env_hash`; a lib edit MUST change it.
//!
//! This is the R21 scoping rule in code.

use std::sync::OnceLock;
use verter_session::file_artifact_store::{
    AugmentationTargetKey, AugmentationTargetKind, FileArtifactKey, ProjectIdentity,
};

use verter_workspace::env_hash::EnvHashInputs;
use verter_workspace::module_resolution::{ConditionSet, ModuleResolutionMode};
use verter_workspace::resolver::{
    IdeProjectCompilerOptions, IdeProjectConfig, ProjectMembership, WorkspaceAlias,
};

fn baseline_conditions() -> &'static ConditionSet {
    static C: OnceLock<ConditionSet> = OnceLock::new();
    C.get_or_init(|| ConditionSet::new(["types", "import", "default"]))
}

fn baseline_cfg() -> IdeProjectConfig {
    let mut cfg = IdeProjectConfig::new(
        "/ws/proj".to_string(),
        "/ws".to_string(),
        Some("/ws/proj/tsconfig.json".to_string()),
    );
    cfg.workspace_aliases = vec![WorkspaceAlias {
        find: "@/".to_string(),
        replacement: "/ws/proj/src/".to_string(),
    }];
    cfg.compiler_options = IdeProjectCompilerOptions {
        base_url: Some("/ws/proj".to_string()),
        paths: vec![("@/*".to_string(), vec!["src/*".to_string()])],
    };
    cfg.references = Vec::new();
    cfg.membership = ProjectMembership::MatchAll;
    cfg
}

fn baseline_inputs() -> EnvHashInputs<'static> {
    EnvHashInputs {
        parser_flags: &["preserve_jsx"],
        resolve_extensions: &[".ts", ".tsx", ".vue"],
        type_strict: true,
        type_no_implicit_any: true,
        lib_names: &["lib.dom.d.ts", "lib.es2022.d.ts"],
        type_roots: &["/ws/node_modules/@types"],
        module_resolution_mode: ModuleResolutionMode::Bundler,
        export_conditions: baseline_conditions(),
        ambient_corpus_fingerprint: 0x1234,
    }
}

/// R21 scoping rule:
/// `FileArtifactKey` is parse-domain only — it MUST NOT change when
/// `lib_env_hash`-only inputs change.
#[test]
fn lib_env_change_does_not_change_file_artifact_key() {
    let cfg = baseline_cfg();
    let baseline = baseline_inputs();
    let parse_env_hash_a = cfg.parse_env_hash(&baseline);

    let mut updated = baseline;
    updated.lib_names = &["lib.dom.d.ts", "lib.es2023.d.ts"]; // bumped lib
    updated.ambient_corpus_fingerprint = 0xfeed;
    let parse_env_hash_b = cfg.parse_env_hash(&updated);

    assert_eq!(
        parse_env_hash_a, parse_env_hash_b,
        "R21: lib_env_hash inputs MUST NOT affect parse_env_hash"
    );

    let canonical: std::sync::Arc<str> = std::sync::Arc::from("/ws/proj/src/foo.ts");
    let content_hash = [0u8; 16];
    let parser_version = 1u32;
    let key_a = FileArtifactKey {
        canonical: canonical.clone(),
        content_hash,
        parse_env_hash: parse_env_hash_a,
        parser_version,
    };
    let key_b = FileArtifactKey {
        canonical,
        content_hash,
        parse_env_hash: parse_env_hash_b,
        parser_version,
    };
    assert_eq!(
        key_a, key_b,
        "R21: lib edit MUST NOT change FileArtifactKey"
    );
}

/// R21 scoping rule:
/// `AugmentationTargetKey` DOES carry `lib_env_hash` — a lib edit MUST
/// change it (so the augmentation index re-resolves).
#[test]
fn lib_env_change_does_change_augmentation_target_key() {
    let cfg = baseline_cfg();
    let baseline = baseline_inputs();
    let lib_a = cfg.lib_env_hash(&baseline);

    let mut updated = baseline;
    updated.lib_names = &["lib.dom.d.ts", "lib.es2023.d.ts"];
    updated.ambient_corpus_fingerprint = 0xfeed;
    let lib_b = cfg.lib_env_hash(&updated);

    assert_ne!(lib_a, lib_b, "lib data change MUST change lib_env_hash");

    let identity = ProjectIdentity(cfg.project_identity());
    let resolve_env_hash = cfg.resolve_env_hash(&baseline);

    let key_a = AugmentationTargetKey {
        project_identity: identity,
        resolve_env_hash,
        lib_env_hash: lib_a,
        population: verter_session::file_artifact_store::AugmentationPopulation::Base,
        target: AugmentationTargetKind::GlobalAugmentation,
    };
    let key_b = AugmentationTargetKey {
        project_identity: identity,
        resolve_env_hash,
        lib_env_hash: lib_b,
        population: verter_session::file_artifact_store::AugmentationPopulation::Base,
        target: AugmentationTargetKind::GlobalAugmentation,
    };
    assert_ne!(
        key_a, key_b,
        "lib edit MUST change AugmentationTargetKey (R21 scoping)"
    );
}

/// R21: `paths` edit changes `resolve_env_hash` and so changes
/// `AugmentationTargetKey`, but does NOT change `FileArtifactKey`.
#[test]
fn paths_edit_changes_augmentation_target_key_but_not_file_artifact_key() {
    let mut cfg = baseline_cfg();
    let inputs = baseline_inputs();
    let parse_a = cfg.parse_env_hash(&inputs);
    let resolve_a = cfg.resolve_env_hash(&inputs);

    cfg.compiler_options.paths = vec![("@/*".to_string(), vec!["lib/*".to_string()])];
    let parse_b = cfg.parse_env_hash(&inputs);
    let resolve_b = cfg.resolve_env_hash(&inputs);
    assert_eq!(
        parse_a, parse_b,
        "paths edit MUST NOT change parse_env_hash"
    );
    assert_ne!(
        resolve_a, resolve_b,
        "paths edit MUST change resolve_env_hash"
    );

    let canonical: std::sync::Arc<str> = std::sync::Arc::from("/ws/proj/src/foo.ts");
    let content_hash = [0u8; 16];
    let parser_version = 1u32;
    let file_key_a = FileArtifactKey {
        canonical: canonical.clone(),
        content_hash,
        parse_env_hash: parse_a,
        parser_version,
    };
    let file_key_b = FileArtifactKey {
        canonical,
        content_hash,
        parse_env_hash: parse_b,
        parser_version,
    };
    assert_eq!(
        file_key_a, file_key_b,
        "paths edit MUST NOT change FileArtifactKey"
    );

    let identity = ProjectIdentity(cfg.project_identity());
    let lib_hash = cfg.lib_env_hash(&inputs);
    let aug_key_a = AugmentationTargetKey {
        project_identity: identity,
        resolve_env_hash: resolve_a,
        lib_env_hash: lib_hash,
        population: verter_session::file_artifact_store::AugmentationPopulation::Base,
        target: AugmentationTargetKind::GlobalAugmentation,
    };
    let aug_key_b = AugmentationTargetKey {
        project_identity: identity,
        resolve_env_hash: resolve_b,
        lib_env_hash: lib_hash,
        population: verter_session::file_artifact_store::AugmentationPopulation::Base,
        target: AugmentationTargetKind::GlobalAugmentation,
    };
    assert_ne!(
        aug_key_a, aug_key_b,
        "paths edit MUST change AugmentationTargetKey (via resolve_env_hash)"
    );
}

/// Two project envs coexist for the same canonical under different keys.
#[test]
fn two_project_envs_coexist_for_same_canonical() {
    let cfg_a = baseline_cfg();
    let mut cfg_b = baseline_cfg();
    cfg_b.compiler_options.paths = vec![("~/*".to_string(), vec!["other/*".to_string()])];

    let inputs = baseline_inputs();
    let canonical: std::sync::Arc<str> = std::sync::Arc::from("/ws/proj/src/foo.ts");
    let content_hash = [42u8; 16];

    let key_a = FileArtifactKey {
        canonical: canonical.clone(),
        content_hash,
        parse_env_hash: cfg_a.parse_env_hash(&inputs),
        parser_version: 1,
    };
    let key_b = FileArtifactKey {
        canonical,
        content_hash,
        parse_env_hash: cfg_b.parse_env_hash(&inputs),
        parser_version: 1,
    };
    // Two configs with identical parser flags but different paths produce
    // the SAME parse_env_hash (parse is independent of resolve / paths).
    assert_eq!(
        key_a, key_b,
        "parse-only env: identical parser flags produce the same FileArtifactKey \
         regardless of paths"
    );

    // But under different parser flags they MUST differ.
    let mut inputs_b = inputs;
    inputs_b.parser_flags = &["preserve_jsx", "vue_macros_v3"];
    let key_c = FileArtifactKey {
        canonical: std::sync::Arc::from("/ws/proj/src/foo.ts"),
        content_hash,
        parse_env_hash: cfg_a.parse_env_hash(&inputs_b),
        parser_version: 1,
    };
    assert_ne!(
        key_a, key_c,
        "two parse envs MUST produce distinct FileArtifactKey entries"
    );
}
