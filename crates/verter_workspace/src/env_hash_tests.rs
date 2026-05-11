//! Stage 1 — env hash 5-way split discriminating tests (R21).
//!
//! These tests assert that each of the five orthogonal env-hash dimensions
//! (`parse_env_hash` / `resolve_env_hash` / `type_env_hash` / `lib_env_hash` /
//! `project_identity`) isolates the correct cache layer.
//!
//! The test surface lives in `verter_workspace` because `IdeProjectConfig` —
//! the source of truth for the resolve-domain / project-identity dimensions —
//! is owned by this crate. The parse, type, and lib dimensions are scoped
//! per call (via the `EnvHashInputs` builder); the bound assertions exercise
//! the discrimination invariant on each dimension.
//!
//! See `docs/arch/fact-based-cache.md` for the audit table that maps every
//! [`IdeProjectConfig`] field to its env-hash dimension.

use super::EnvHashInputs;
use crate::resolver::{
    IdeProjectCompilerOptions, IdeProjectConfig, ProjectMembership, WorkspaceAlias,
};

/// Helper to construct a baseline `(IdeProjectConfig, EnvHashInputs)` pair.
fn baseline() -> (IdeProjectConfig, EnvHashInputs<'static>) {
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
    cfg.references = vec!["/ws/proj/tsconfig.refs.json".to_string()];
    cfg.membership = ProjectMembership::IncludeExclude {
        files: vec!["index.ts".to_string()],
        include: vec!["src/**".to_string()],
        exclude: vec!["dist/**".to_string()],
    };
    let inputs = EnvHashInputs {
        parser_flags: &["preserve_jsx", "vue_macros_v3"],
        resolve_extensions: &[".ts", ".tsx", ".vue"],
        type_strict: true,
        type_no_implicit_any: true,
        lib_names: &["lib.dom.d.ts", "lib.es2022.d.ts"],
        type_roots: &["/ws/node_modules/@types"],
        ambient_corpus_fingerprint: 0x0123_4567_89ab_cdef,
    };
    (cfg, inputs)
}

// ── parse_env_hash discrimination ──

#[test]
fn parse_env_hash_changes_when_parser_flag_flips() {
    let (cfg, mut inputs) = baseline();
    let h0 = cfg.parse_env_hash(&inputs);
    inputs.parser_flags = &["preserve_jsx"]; // dropped vue_macros_v3
    let h1 = cfg.parse_env_hash(&inputs);
    assert_ne!(h0, h1, "parser flag flip MUST change parse_env_hash");

    // The other 4 dimensions stay identical.
    let (cfg2, mut inputs2) = baseline();
    let r0 = cfg.resolve_env_hash(&baseline().1);
    let r1 = cfg2.resolve_env_hash({
        inputs2.parser_flags = &["preserve_jsx"];
        &inputs2
    });
    assert_eq!(r0, r1, "parser flag flip MUST NOT change resolve_env_hash");
    let (cfg3, mut inputs3) = baseline();
    let t0 = cfg.type_env_hash(&baseline().1);
    let t1 = cfg3.type_env_hash({
        inputs3.parser_flags = &["preserve_jsx"];
        &inputs3
    });
    assert_eq!(t0, t1, "parser flag flip MUST NOT change type_env_hash");
    let (cfg4, mut inputs4) = baseline();
    let l0 = cfg.lib_env_hash(&baseline().1);
    let l1 = cfg4.lib_env_hash({
        inputs4.parser_flags = &["preserve_jsx"];
        &inputs4
    });
    assert_eq!(l0, l1, "parser flag flip MUST NOT change lib_env_hash");
    let p0 = cfg.project_identity();
    let p1 = baseline().0.project_identity();
    assert_eq!(
        p0, p1,
        "parser flag flip MUST NOT change project_identity (parser flags are not project identity)"
    );
}

// ── resolve_env_hash discrimination ──

#[test]
fn resolve_env_hash_changes_when_paths_changes() {
    let (mut cfg, inputs) = baseline();
    let h0 = cfg.resolve_env_hash(&inputs);
    cfg.compiler_options.paths = vec![("@/*".to_string(), vec!["lib/*".to_string()])];
    let h1 = cfg.resolve_env_hash(&inputs);
    assert_ne!(h0, h1, "paths edit MUST change resolve_env_hash");

    // The other 4 dimensions stay identical.
    let (mut cfg2, inputs2) = baseline();
    let p0 = baseline().0.parse_env_hash(&inputs2);
    cfg2.compiler_options.paths = vec![("@/*".to_string(), vec!["lib/*".to_string()])];
    let p1 = cfg2.parse_env_hash(&inputs2);
    assert_eq!(p0, p1, "paths edit MUST NOT change parse_env_hash");

    let (mut cfg3, _) = baseline();
    let t0 = baseline().0.type_env_hash(&inputs2);
    cfg3.compiler_options.paths = vec![("@/*".to_string(), vec!["lib/*".to_string()])];
    let t1 = cfg3.type_env_hash(&inputs2);
    assert_eq!(t0, t1, "paths edit MUST NOT change type_env_hash");

    let (mut cfg4, _) = baseline();
    let l0 = baseline().0.lib_env_hash(&inputs2);
    cfg4.compiler_options.paths = vec![("@/*".to_string(), vec!["lib/*".to_string()])];
    let l1 = cfg4.lib_env_hash(&inputs2);
    assert_eq!(l0, l1, "paths edit MUST NOT change lib_env_hash");

    let (mut cfg5, _) = baseline();
    let i0 = baseline().0.project_identity();
    cfg5.compiler_options.paths = vec![("@/*".to_string(), vec!["lib/*".to_string()])];
    let i1 = cfg5.project_identity();
    assert_eq!(i0, i1, "paths edit MUST NOT change project_identity");
}

#[test]
fn resolve_env_hash_changes_when_workspace_aliases_changes() {
    let (mut cfg, inputs) = baseline();
    let h0 = cfg.resolve_env_hash(&inputs);
    cfg.workspace_aliases.push(WorkspaceAlias {
        find: "~/".to_string(),
        replacement: "/ws/proj/test/".to_string(),
    });
    let h1 = cfg.resolve_env_hash(&inputs);
    assert_ne!(
        h0, h1,
        "workspace_aliases edit MUST change resolve_env_hash"
    );
}

#[test]
fn resolve_env_hash_changes_when_resolve_extensions_changes() {
    let (cfg, mut inputs) = baseline();
    let h0 = cfg.resolve_env_hash(&inputs);
    inputs.resolve_extensions = &[".ts", ".tsx"]; // dropped .vue
    let h1 = cfg.resolve_env_hash(&inputs);
    assert_ne!(
        h0, h1,
        "resolve_extensions edit MUST change resolve_env_hash"
    );
}

#[test]
fn resolve_env_hash_changes_when_references_changes() {
    let (mut cfg, inputs) = baseline();
    let h0 = cfg.resolve_env_hash(&inputs);
    cfg.references
        .push("/ws/proj/tsconfig.extra.json".to_string());
    let h1 = cfg.resolve_env_hash(&inputs);
    assert_ne!(
        h0, h1,
        "project references edit MUST change resolve_env_hash"
    );
}

// ── type_env_hash discrimination ──

#[test]
fn type_env_hash_changes_when_strict_flips() {
    let (cfg, mut inputs) = baseline();
    let h0 = cfg.type_env_hash(&inputs);
    inputs.type_strict = false;
    let h1 = cfg.type_env_hash(&inputs);
    assert_ne!(h0, h1, "strict flip MUST change type_env_hash");

    // The other 4 dimensions stay identical.
    let r0 = cfg.resolve_env_hash(&baseline().1);
    let r1 = cfg.resolve_env_hash(&inputs);
    assert_eq!(r0, r1, "strict flip MUST NOT change resolve_env_hash");
    let p0 = cfg.parse_env_hash(&baseline().1);
    let p1 = cfg.parse_env_hash(&inputs);
    assert_eq!(p0, p1, "strict flip MUST NOT change parse_env_hash");
    let l0 = cfg.lib_env_hash(&baseline().1);
    let l1 = cfg.lib_env_hash(&inputs);
    assert_eq!(l0, l1, "strict flip MUST NOT change lib_env_hash");
}

// ── lib_env_hash discrimination ──

#[test]
fn lib_env_hash_changes_when_lib_names_changes() {
    let (cfg, mut inputs) = baseline();
    let h0 = cfg.lib_env_hash(&inputs);
    inputs.lib_names = &["lib.dom.d.ts", "lib.es2023.d.ts"]; // ES bump
    let h1 = cfg.lib_env_hash(&inputs);
    assert_ne!(h0, h1, "lib names edit MUST change lib_env_hash");

    // CRITICAL R21 scoping rule: lib_names changing MUST NOT change resolve_env_hash.
    let r0 = cfg.resolve_env_hash(&baseline().1);
    let r1 = cfg.resolve_env_hash(&inputs);
    assert_eq!(
        r0, r1,
        "R21 scoping rule: TS lib change MUST NOT change resolve_env_hash"
    );
    // Also: parse_env_hash and project_identity are independent of libs.
    let p0 = cfg.parse_env_hash(&baseline().1);
    let p1 = cfg.parse_env_hash(&inputs);
    assert_eq!(p0, p1, "lib names edit MUST NOT change parse_env_hash");
}

#[test]
fn lib_env_hash_changes_when_ambient_corpus_changes() {
    let (cfg, mut inputs) = baseline();
    let h0 = cfg.lib_env_hash(&inputs);
    inputs.ambient_corpus_fingerprint = 0xdead_beef_dead_beef;
    let h1 = cfg.lib_env_hash(&inputs);
    assert_ne!(
        h0, h1,
        "ambient corpus fingerprint MUST change lib_env_hash"
    );

    // R21 scoping rule applies here too.
    let r0 = cfg.resolve_env_hash(&baseline().1);
    let r1 = cfg.resolve_env_hash(&inputs);
    assert_eq!(
        r0, r1,
        "R21: ambient corpus change MUST NOT change resolve_env_hash"
    );
}

// ── project_identity discrimination ──

#[test]
fn project_identity_changes_when_root_changes() {
    let (cfg, _) = baseline();
    let h0 = cfg.project_identity();
    let mut cfg2 = cfg.clone();
    cfg2.root = "/ws/other-proj".to_string();
    let h1 = cfg2.project_identity();
    assert_ne!(h0, h1, "project root change MUST change project_identity");
}

#[test]
fn project_identity_changes_when_tsconfig_path_changes() {
    let (cfg, _) = baseline();
    let h0 = cfg.project_identity();
    let mut cfg2 = cfg.clone();
    cfg2.tsconfig_path = Some("/ws/proj/tsconfig.app.json".to_string());
    let h1 = cfg2.project_identity();
    assert_ne!(h0, h1, "tsconfig_path change MUST change project_identity");
}

#[test]
fn project_identity_changes_when_workspace_root_changes() {
    let (cfg, _) = baseline();
    let h0 = cfg.project_identity();
    let mut cfg2 = cfg.clone();
    cfg2.workspace_root = "/other-ws".to_string();
    let h1 = cfg2.project_identity();
    assert_ne!(h0, h1, "workspace_root change MUST change project_identity");
}

#[test]
fn project_identity_changes_when_provider_root_changes() {
    let (cfg, _) = baseline();
    let h0 = cfg.project_identity();
    let mut cfg2 = cfg.clone();
    cfg2.provider_root = "/ws/other-provider".to_string();
    let h1 = cfg2.project_identity();
    assert_ne!(h0, h1, "provider_root change MUST change project_identity");
}

#[test]
fn project_identity_changes_when_membership_changes() {
    let (cfg, _) = baseline();
    let h0 = cfg.project_identity();
    let mut cfg2 = cfg.clone();
    cfg2.membership = ProjectMembership::MatchAll;
    let h1 = cfg2.project_identity();
    assert_ne!(h0, h1, "membership change MUST change project_identity");
}

// ── Determinism + stability ──

#[test]
fn env_hashes_are_deterministic_across_calls() {
    let (cfg, inputs) = baseline();
    assert_eq!(cfg.parse_env_hash(&inputs), cfg.parse_env_hash(&inputs));
    assert_eq!(cfg.resolve_env_hash(&inputs), cfg.resolve_env_hash(&inputs));
    assert_eq!(cfg.type_env_hash(&inputs), cfg.type_env_hash(&inputs));
    assert_eq!(cfg.lib_env_hash(&inputs), cfg.lib_env_hash(&inputs));
    assert_eq!(cfg.project_identity(), cfg.project_identity());
}

#[test]
fn env_hashes_distinguish_across_dimensions() {
    // The 5 hashes derived from the same baseline MUST NOT collide.
    // (They derive different field subsets + carry a per-dimension salt.)
    let (cfg, inputs) = baseline();
    let p = cfg.parse_env_hash(&inputs);
    let r = cfg.resolve_env_hash(&inputs);
    let t = cfg.type_env_hash(&inputs);
    let l = cfg.lib_env_hash(&inputs);
    let i = cfg.project_identity();
    let all = [p, r, t, l, i];
    for x in 0..all.len() {
        for y in (x + 1)..all.len() {
            assert_ne!(
                all[x], all[y],
                "dimension salts MUST make distinct dimensions hash differently"
            );
        }
    }
}
