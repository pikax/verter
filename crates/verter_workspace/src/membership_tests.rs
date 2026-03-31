use super::*;
use crate::canonical_path::CanonicalPath;
use crate::normalized_glob::NormalizedGlob;

fn root() -> CanonicalPath {
    CanonicalPath::new("d:/project")
}

fn make_spec(files: &[&str], include: &[&str], exclude: &[&str]) -> StaticMembershipSpec {
    StaticMembershipSpec {
        files: files.iter().map(|s| CanonicalPath::new(s)).collect(),
        include: include.iter().map(|s| NormalizedGlob::new(s)).collect(),
        exclude: exclude.iter().map(|s| NormalizedGlob::new(s)).collect(),
    }
}

// ── BUG FIX: files immune to exclude ──
//
// The old matches_file() checked exclude BEFORE files, causing files
// entries to be incorrectly excluded. This test verifies the fix.

#[test]
fn files_immune_to_exclude() {
    let spec = make_spec(
        &["d:/project/src/main.ts"],
        &["d:/project/src/**/*"],
        &["d:/project/src/**/*"], // exclude everything — but files should still match
    );

    let path = CanonicalPath::new("d:/project/src/main.ts");
    assert!(
        spec.matches(&path),
        "files entries MUST be immune to exclude"
    );
}

#[test]
fn files_immune_to_exclude_even_specific_pattern() {
    let spec = make_spec(
        &["d:/project/generated/types.ts"],
        &[],
        &["d:/project/generated/**"],
    );

    let path = CanonicalPath::new("d:/project/generated/types.ts");
    assert!(
        spec.matches(&path),
        "files entry should match even when exclude covers its directory"
    );
}

// ── No filters → TypeScript defaults ──

#[test]
fn no_filters_uses_typescript_defaults() {
    let spec = StaticMembershipSpec::with_typescript_defaults(&root());

    // Should match normal source files
    assert!(spec.matches(&CanonicalPath::new("d:/project/src/foo.ts")));
    assert!(spec.matches(&CanonicalPath::new("d:/project/lib/bar.vue")));
}

#[test]
fn no_filters_excludes_node_modules() {
    let spec = StaticMembershipSpec::with_typescript_defaults(&root());

    // Default exclude should exclude node_modules
    assert!(
        !spec.matches(&CanonicalPath::new("d:/project/node_modules/vue/index.ts")),
        "default exclude should filter node_modules"
    );
}

#[test]
fn no_filters_excludes_bower_components() {
    let spec = StaticMembershipSpec::with_typescript_defaults(&root());

    assert!(
        !spec.matches(&CanonicalPath::new(
            "d:/project/bower_components/jquery/jquery.js"
        )),
        "default exclude should filter bower_components"
    );
}

// ── files only (no include) ──

#[test]
fn files_only_matches_exactly() {
    let spec = make_spec(
        &["d:/project/src/main.ts"],
        &[], // no include
        &[], // no exclude
    );

    assert!(spec.matches(&CanonicalPath::new("d:/project/src/main.ts")));
    assert!(
        !spec.matches(&CanonicalPath::new("d:/project/src/other.ts")),
        "non-listed file should not match"
    );
}

#[test]
fn files_present_but_include_absent_no_implicit_include() {
    let spec = make_spec(
        &["d:/project/src/main.ts"],
        &[], // no include — should NOT default to **/*
        &[],
    );

    // Only the listed file matches
    assert!(spec.matches(&CanonicalPath::new("d:/project/src/main.ts")));
    // Other files under the root should NOT match (no implicit include)
    assert!(
        !spec.matches(&CanonicalPath::new("d:/project/src/other.ts")),
        "when files is present but include is absent, no implicit include"
    );
}

// ── include + exclude ──

#[test]
fn include_minus_exclude() {
    let spec = make_spec(
        &[],
        &["d:/project/src/**/*"],
        &["d:/project/src/generated/**"],
    );

    assert!(spec.matches(&CanonicalPath::new("d:/project/src/foo.ts")));
    assert!(
        !spec.matches(&CanonicalPath::new("d:/project/src/generated/types.ts")),
        "exclude should filter out generated directory"
    );
}

#[test]
fn exclude_only_filters_include() {
    // exclude without include should NOT cause anything to match
    let spec = make_spec(
        &[],
        &[], // no include, no files
        &["d:/project/dist/**"],
    );

    assert!(
        !spec.matches(&CanonicalPath::new("d:/project/src/foo.ts")),
        "exclude alone should not imply include"
    );
}

// ── Solution-style ──

#[test]
fn solution_style_empty_files_matches_nothing() {
    // { "files": [], "references": [...] } → matches nothing
    let spec = make_spec(
        &[], // empty files
        &[], // no include
        &[], // no exclude
    );

    assert!(
        !spec.matches(&CanonicalPath::new("d:/project/src/foo.ts")),
        "solution-style tsconfig should match nothing"
    );
}

// ── Configured membership ──

#[test]
fn configured_membership_contains() {
    let mut materialized = FxHashSet::default();
    materialized.insert(CanonicalPath::new("d:/project/src/main.ts"));
    materialized.insert(CanonicalPath::new("d:/project/src/app.vue"));

    let membership = ConfiguredMembership {
        spec: StaticMembershipSpec::with_typescript_defaults(&root()),
        materialized_files: materialized,
    };

    assert!(membership.contains(&CanonicalPath::new("d:/project/src/main.ts")));
    assert!(membership.contains(&CanonicalPath::new("d:/project/src/app.vue")));
    assert!(
        !membership.contains(&CanonicalPath::new("d:/project/src/other.ts")),
        "non-materialized file should not be contained"
    );
}

// ── Fallback membership ──

#[test]
fn fallback_contains_under_root() {
    let membership = FallbackMembership {
        root: root(),
        exclude: vec![NormalizedGlob::new("d:/project/node_modules/**")],
    };

    assert!(membership.contains(&CanonicalPath::new("d:/project/src/foo.ts")));
    assert!(membership.contains(&CanonicalPath::new("d:/project/deep/nested/bar.vue")));
}

#[test]
fn fallback_rejects_outside_root() {
    let membership = FallbackMembership {
        root: root(),
        exclude: vec![],
    };

    assert!(
        !membership.contains(&CanonicalPath::new("d:/other/src/foo.ts")),
        "file outside root should not match fallback"
    );
}

#[test]
fn fallback_rejects_excluded() {
    let membership = FallbackMembership {
        root: root(),
        exclude: vec![NormalizedGlob::new("d:/project/node_modules/**")],
    };

    assert!(
        !membership.contains(&CanonicalPath::new("d:/project/node_modules/vue/index.ts")),
        "excluded path should not match fallback"
    );
}

#[test]
fn fallback_rejects_partial_prefix() {
    let membership = FallbackMembership {
        root: root(),
        exclude: vec![],
    };

    assert!(
        !membership.contains(&CanonicalPath::new("d:/project-extra/foo.ts")),
        "project-extra should not match project root"
    );
}

#[test]
fn fallback_has_no_configured_settings() {
    // Type system enforces this — FallbackMembership has no tsconfig_path,
    // no compiler_options, no workspace_aliases fields.
    // This test just documents the invariant.
    let membership = FallbackMembership {
        root: root(),
        exclude: vec![],
    };
    // There's no way to access tsconfig settings from a FallbackMembership
    // because the type simply doesn't have those fields.
    assert!(membership.contains(&CanonicalPath::new("d:/project/foo.ts")));
}

// ── exclude-only (no files, no include) ──

#[test]
fn exclude_only_no_files_no_include_matches_nothing() {
    // Per the plan: "exclude-only (no files, no include) → include defaults to ["**/*"]"
    // BUT this only applies when the BUILDER fills in defaults.
    // The raw spec with empty files + empty include + non-empty exclude matches nothing.
    let spec = make_spec(&[], &[], &["d:/project/dist/**"]);
    assert!(
        !spec.matches(&CanonicalPath::new("d:/project/src/foo.ts")),
        "raw spec with no files and no include should match nothing"
    );
}

// ── Bridge fallback (empty materialized set with non-empty spec) ──

#[test]
fn configured_membership_bridge_fallback_when_empty_materialized() {
    // When materialized_files is empty but spec has include patterns,
    // contains() should fall back to spec.matches() (bridge mode).
    let spec = StaticMembershipSpec::with_typescript_defaults(&root());

    let membership = ConfiguredMembership {
        spec,
        materialized_files: FxHashSet::default(), // empty → bridge mode
    };

    // Bridge mode uses spec.matches(), which includes default "**/*" pattern
    assert!(
        membership.contains(&CanonicalPath::new("d:/project/src/foo.ts")),
        "bridge mode should fall back to spec.matches()"
    );

    // But node_modules should still be excluded by default TS excludes
    assert!(
        !membership.contains(&CanonicalPath::new("d:/project/node_modules/vue/index.ts")),
        "bridge mode should still respect exclude patterns"
    );
}

#[test]
fn configured_membership_prefers_materialized_when_populated() {
    let spec = StaticMembershipSpec::with_typescript_defaults(&root());

    let mut materialized = FxHashSet::default();
    materialized.insert(CanonicalPath::new("d:/project/src/main.ts"));

    let membership = ConfiguredMembership {
        spec,
        materialized_files: materialized,
    };

    // Materialized file is found
    assert!(membership.contains(&CanonicalPath::new("d:/project/src/main.ts")));

    // Non-materialized file is NOT found (even though spec.matches would say yes)
    assert!(
        !membership.contains(&CanonicalPath::new("d:/project/src/other.ts")),
        "should use materialized set, not spec fallback"
    );
}
