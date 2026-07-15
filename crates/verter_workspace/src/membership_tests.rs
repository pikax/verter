use super::*;
use crate::canonical_path::CanonicalPath;
use crate::normalized_glob::{CompiledGlob, NormalizedGlob};

fn root() -> CanonicalPath {
    CanonicalPath::new("d:/project")
}

fn compiled(raw: &str) -> CompiledGlob {
    CompiledGlob::new(NormalizedGlob::new(raw))
}

fn make_spec(files: &[&str], include: &[&str], exclude: &[&str]) -> StaticMembershipSpec {
    StaticMembershipSpec {
        files: files.iter().map(|s| CanonicalPath::new(s)).collect(),
        include: include.iter().map(|s| compiled(s)).collect(),
        exclude: exclude.iter().map(|s| compiled(s)).collect(),
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
        exclude: vec![compiled("d:/project/node_modules/**")],
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
        exclude: vec![compiled("d:/project/node_modules/**")],
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

// ── Invalid glob patterns (pin: invalid pattern → no match, no panic) ──

#[test]
fn invalid_include_glob_yields_no_membership() {
    // Self-check: the fixture must actually be an invalid glob pattern.
    assert!(glob::Pattern::new("d:/project/src/[unclosed").is_err());

    let spec = make_spec(&[], &["d:/project/src/[unclosed"], &[]);
    assert!(
        !spec.matches(&CanonicalPath::new("d:/project/src/foo.ts")),
        "invalid include pattern must never match"
    );

    // Same through ConfiguredMembership bridge mode (empty materialized set).
    let membership = ConfiguredMembership {
        spec: make_spec(&[], &["d:/project/src/[unclosed"], &[]),
        materialized_files: FxHashSet::default(),
    };
    assert!(
        !membership.contains(&CanonicalPath::new("d:/project/src/foo.ts")),
        "invalid include pattern must yield contains == false"
    );
}

#[test]
fn invalid_exclude_glob_never_excludes() {
    assert!(glob::Pattern::new("d:/project/[unclosed").is_err());

    // Valid include, invalid exclude → include wins, no panic.
    let spec = make_spec(&[], &["d:/project/**/*"], &["d:/project/[unclosed"]);
    assert!(
        spec.matches(&CanonicalPath::new("d:/project/src/foo.ts")),
        "invalid exclude pattern must not exclude included files"
    );
}

#[test]
fn fallback_invalid_exclude_glob_never_excludes() {
    assert!(glob::Pattern::new("d:/project/[unclosed").is_err());

    let membership = FallbackMembership {
        root: root(),
        exclude: vec![compiled("d:/project/[unclosed")],
    };
    assert!(
        membership.contains(&CanonicalPath::new("d:/project/src/foo.ts")),
        "invalid exclude pattern must not reject files under root"
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

// ─────────────────────────────────────────────────────────────────────────
// Supported-extension membership model (TS-equivalent extension expansion).
//
// TypeScript's default `include` is `**/*` filtered to KNOWN extensions
// (`.ts`/`.tsx`/`.d.ts`, `.js`/`.jsx` iff `allowJs`, plus declared
// `extraFileExtensions`). A no-extension directory glob (`"src"`, resolved to
// `"src/**/*"`) or a bare-star glob expands into one glob per supported
// extension; an extension-specific glob (`"src/**/*.ts"`) is never expanded.
//
// The carrier extensions act as `extraFileExtensions` and are sourced from
// `verter_language::LanguageRegistry::global().carrier_extensions()` — NEVER
// hardcoded — so the rule is adapter-parameterized (`.vue` AND `.svelte`).
// ─────────────────────────────────────────────────────────────────────────

/// The supported-extension set the live `LanguageRegistry` exposes: at least
/// `.vue` AND `.svelte` are registered carrier extensions. The membership model
/// uses these (no literal `"vue"`/`"svelte"`).
fn registered_carrier_exts() -> Vec<String> {
    verter_language::LanguageRegistry::global()
        .carrier_extensions()
        .iter()
        .map(|e| (*e).to_string())
        .collect()
}

/// Build a configured spec from raw `include` globs the way the snapshot builder
/// does: resolve each through `resolve_membership_path` (so a bare directory
/// `"src"` becomes `"src/**/*"`), then apply the supported-extension expansion.
fn spec_from_includes(
    includes: &[&str],
    allow_js: bool,
    carrier_exts: &[String],
) -> StaticMembershipSpec {
    let r = root();
    let supported = SupportedExtensions::new(allow_js, carrier_exts);
    let resolved: Vec<String> = includes
        .iter()
        .map(|inc| crate::config::resolve_membership_path(r.as_str(), inc, true))
        .collect();
    let resolved_refs: Vec<&str> = resolved.iter().map(String::as_str).collect();
    StaticMembershipSpec::from_includes(&r, &[], &resolved_refs, &[], &supported)
}

/// Case 1 — `include: ["src"]` (directory glob) OWNS `src/Foo.vue` AND
/// `src/Foo.svelte` (adapter-parameterized over every registered carrier).
#[test]
fn directory_glob_owns_every_carrier_source() {
    let carriers = registered_carrier_exts();
    let spec = spec_from_includes(&["src"], false, &carriers);
    for ext in &carriers {
        let path = CanonicalPath::new(&format!("d:/project/src/Foo.{ext}"));
        assert!(
            spec.matches(&path),
            "a no-extension directory glob (`src`) must own a carrier source `Foo.{ext}`"
        );
    }
    // It also owns ordinary TS.
    assert!(spec.matches(&CanonicalPath::new("d:/project/src/util.ts")));
}

/// Case 2 — `include: ["src/**/*"]` (bare star) OWNS the carrier sources.
#[test]
fn bare_star_glob_owns_every_carrier_source() {
    let carriers = registered_carrier_exts();
    let spec = spec_from_includes(&["src/**/*"], false, &carriers);
    for ext in &carriers {
        let path = CanonicalPath::new(&format!("d:/project/src/Foo.{ext}"));
        assert!(
            spec.matches(&path),
            "a bare-star glob (`src/**/*`) must own a carrier source `Foo.{ext}`"
        );
    }
}

/// Case 3 — `include: ["src/**/*.ts"]` (extension-specific) does NOT own the
/// carrier sources (it is never expanded). This is the `NoProject` case.
#[test]
fn extension_specific_glob_does_not_own_carrier_source() {
    let carriers = registered_carrier_exts();
    let spec = spec_from_includes(&["src/**/*.ts"], false, &carriers);
    for ext in &carriers {
        let path = CanonicalPath::new(&format!("d:/project/src/Foo.{ext}"));
        assert!(
            !spec.matches(&path),
            "an extension-specific glob (`src/**/*.ts`) must NOT own a carrier source `Foo.{ext}`"
        );
    }
    // It DOES still own the `.ts` it names.
    assert!(spec.matches(&CanonicalPath::new("d:/project/src/util.ts")));
}

/// Case 4 — SEPARATE per-extension entries
/// (`["src/**/*.ts", "src/**/*.vue", "src/**/*.svelte"]`) OWN the carrier
/// sources. NO brace expansion — never `*.{vue,svelte}`.
#[test]
fn separate_per_extension_entries_own_carrier_sources() {
    let carriers = registered_carrier_exts();
    let mut includes: Vec<String> = vec!["src/**/*.ts".to_string()];
    for ext in &carriers {
        includes.push(format!("src/**/*.{ext}"));
    }
    let include_refs: Vec<&str> = includes.iter().map(String::as_str).collect();
    let spec = spec_from_includes(&include_refs, false, &carriers);
    for ext in &carriers {
        let path = CanonicalPath::new(&format!("d:/project/src/Foo.{ext}"));
        assert!(
            spec.matches(&path),
            "a separate per-extension entry `src/**/*.{ext}` must own `Foo.{ext}`"
        );
    }
    assert!(spec.matches(&CanonicalPath::new("d:/project/src/util.ts")));
}

/// Case 5 — default-include (no `files`/`include`) OWNS the carrier sources.
#[test]
fn default_include_owns_every_carrier_source() {
    let carriers = registered_carrier_exts();
    let supported = SupportedExtensions::new(false, &carriers);
    let spec = StaticMembershipSpec::with_supported_extension_defaults(&root(), &supported);
    for ext in &carriers {
        let path = CanonicalPath::new(&format!("d:/project/src/Foo.{ext}"));
        assert!(
            spec.matches(&path),
            "the default include must own a carrier source `Foo.{ext}`"
        );
    }
    assert!(spec.matches(&CanonicalPath::new("d:/project/src/deep/util.ts")));
    // Default exclude still filters node_modules.
    assert!(!spec.matches(&CanonicalPath::new("d:/project/node_modules/vue/index.ts")));
}

/// Case 6 — an `exclude`d carrier source ⇒ NOT owned (`NoProject`). `exclude`
/// is literal / extension-agnostic; it filters the expanded include set.
#[test]
fn excluded_carrier_source_is_not_owned() {
    let carriers = registered_carrier_exts();
    let r = root();
    let supported = SupportedExtensions::new(false, &carriers);
    let spec = StaticMembershipSpec::from_includes(
        &r,
        &[],
        &["d:/project/src/**/*"],
        &["d:/project/src/generated/**"],
        &supported,
    );
    for ext in &carriers {
        // A carrier under the excluded dir is NOT owned.
        assert!(
            !spec.matches(&CanonicalPath::new(&format!(
                "d:/project/src/generated/Foo.{ext}"
            ))),
            "an excluded carrier source `generated/Foo.{ext}` must NOT be owned"
        );
        // But a carrier outside the excluded dir IS owned.
        assert!(
            spec.matches(&CanonicalPath::new(&format!("d:/project/src/Foo.{ext}"))),
            "a non-excluded carrier source `Foo.{ext}` is still owned"
        );
    }
}

/// NEGATIVE CONTROL (discriminates the explicit-extension model from today's
/// literal-glob accident): under a bare-star glob, an UNKNOWN, non-carrier,
/// non-supported extension (`src/Foo.foo`) is NOT owned. Today's literal-glob
/// `**/*` would wrongly own it — so this test is RED without the model.
#[test]
fn bare_star_does_not_own_unknown_extension() {
    let carriers = registered_carrier_exts();
    let spec = spec_from_includes(&["src/**/*"], false, &carriers);
    assert!(
        !spec.matches(&CanonicalPath::new("d:/project/src/Foo.foo")),
        "a bare-star glob must NOT own an unknown non-carrier extension `Foo.foo` \
         (this discriminates the explicit supported-extension model from the \
         literal-glob accident)"
    );
    // Sanity: a `.foo`-specific glob WOULD own it (extension-specific, never expanded,
    // matched literally) — proving the model only filters the EXPANDED set.
    let r = root();
    let supported = SupportedExtensions::new(false, &carriers);
    let explicit =
        StaticMembershipSpec::from_includes(&r, &[], &["d:/project/src/**/*.foo"], &[], &supported);
    assert!(
        explicit.matches(&CanonicalPath::new("d:/project/src/Foo.foo")),
        "an extension-specific `.foo` glob is matched literally (never expanded)"
    );
}

/// `allowJs`/`checkJs` gating: under a bare-star glob, a `.js` file is owned
/// ONLY when allowJs/checkJs is set (RED without the model — today's literal
/// `**/*` owns `.js` unconditionally).
#[test]
fn bare_star_owns_js_only_when_allow_js() {
    let carriers = registered_carrier_exts();
    let js = CanonicalPath::new("d:/project/src/util.js");

    // allowJs OFF → `.js` is NOT a supported extension → not owned.
    let no_js = spec_from_includes(&["src/**/*"], false, &carriers);
    assert!(
        !no_js.matches(&js),
        "without allowJs/checkJs, a bare-star glob must NOT own a `.js` file"
    );

    // allowJs ON → `.js` IS supported → owned.
    let with_js = spec_from_includes(&["src/**/*"], true, &carriers);
    assert!(
        with_js.matches(&js),
        "with allowJs/checkJs, a bare-star glob owns a `.js` file"
    );
}

/// The standard TS extension set is always present regardless of allowJs and
/// regardless of which carriers are registered. Discriminates a model that
/// forgot `.d.ts` / `.mts` / `.cts` family members.
#[test]
fn supported_set_always_includes_full_ts_family() {
    let supported = SupportedExtensions::new(false, &[]);
    let exts = supported.extensions();
    for required in [".ts", ".tsx", ".d.ts", ".cts", ".mts", ".d.cts", ".d.mts"] {
        assert!(
            exts.iter().any(|e| e == required),
            "the supported-extension set must always include `{required}`, got {exts:?}"
        );
    }
    // JS family absent when allowJs is off.
    for js in [".js", ".jsx", ".cjs", ".mjs"] {
        assert!(
            !exts.iter().any(|e| e == js),
            "the JS family must be absent without allowJs/checkJs, but `{js}` was present"
        );
    }
    // With allowJs the JS family appears.
    let with_js = SupportedExtensions::new(true, &[]);
    for js in [".js", ".jsx", ".cjs", ".mjs"] {
        assert!(
            with_js.extensions().iter().any(|e| e == js),
            "with allowJs the supported set must include `{js}`"
        );
    }
}
