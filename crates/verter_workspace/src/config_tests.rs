use super::*;

// ═══════════════════════════════════════════════════════════════════════════
// strip_json_comments
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn strip_single_line_comments() {
    let input = r#"{
  // This is a comment
  "baseUrl": "."
}"#;
    let result = strip_json_comments(input);
    assert!(
        result.contains(r#""baseUrl": ".""#),
        "should preserve properties"
    );
    assert!(!result.contains("//"), "should remove single-line comments");
}

#[test]
fn strip_multi_line_comments() {
    let input = r#"{
  /* multi
     line
     comment */
  "baseUrl": "."
}"#;
    let result = strip_json_comments(input);
    assert!(
        result.contains(r#""baseUrl": ".""#),
        "should preserve properties"
    );
    assert!(
        !result.contains("/*"),
        "should remove multi-line comment start"
    );
    assert!(
        !result.contains("*/"),
        "should remove multi-line comment end"
    );
}

#[test]
fn strip_inline_comment() {
    let input = r#"{ "baseUrl": "." /* inline */ }"#;
    let result = strip_json_comments(input);
    assert!(result.contains(r#""baseUrl": ".""#));
    assert!(!result.contains("inline"), "should remove inline comment");
}

#[test]
fn preserve_strings_with_slashes() {
    let input = r#"{ "url": "http://example.com" }"#;
    let result = strip_json_comments(input);
    assert!(
        result.contains("http://example.com"),
        "should preserve // inside strings"
    );
}

#[test]
fn strip_trailing_comma_before_brace() {
    let input = r#"{ "a": 1, }"#;
    let result = strip_json_comments(input);
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert_eq!(
        parsed["a"], 1,
        "should be valid JSON after stripping trailing comma"
    );
}

#[test]
fn strip_trailing_comma_before_bracket() {
    let input = r#"[1, 2, 3, ]"#;
    let result = strip_json_comments(input);
    let parsed: serde_json::Value = serde_json::from_str(&result).unwrap();
    assert!(parsed.is_array());
    assert_eq!(parsed.as_array().unwrap().len(), 3);
}

#[test]
fn full_tsconfig_with_comments_and_trailing_commas() {
    let input = r#"{
  // TypeScript config
  "compilerOptions": {
    "baseUrl": ".",
    "paths": {
      "@/*": ["./src/*"], // Source alias
    },
  },
  "include": ["src/**/*"],
  /* Exclude tests */
  "exclude": ["node_modules",],
}"#;
    let result = strip_json_comments(input);
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&result);
    assert!(
        parsed.is_ok(),
        "should produce valid JSON: {:?}",
        parsed.err()
    );
    let json = parsed.unwrap();
    assert_eq!(
        json["compilerOptions"]["baseUrl"].as_str(),
        Some("."),
        "should preserve baseUrl"
    );
    // Negative: no comments in output (but note @/* is a valid path pattern)
    assert!(!result.contains("// "), "no single-line comments in output");
    assert!(
        !result.contains("/* "),
        "no multi-line comment starts in output"
    );
    assert!(
        !result.contains("Exclude tests"),
        "multi-line comment content should be removed"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// discover_tsconfigs
// ═══════════════════════════════════════════════════════════════════════════

// Note: tempfile::TempDir on Windows creates dirs like `.tmpXXX` which
// start with a dot. The discover function filters dot-directories, so we
// must create a non-dot subdirectory as the workspace root for tests.

#[test]
fn discover_finds_root_tsconfig() {
    let tmp = tempfile::TempDir::new().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("tsconfig.json"), "{}").unwrap();

    let entries = discover_tsconfigs(&workspace);
    assert_eq!(entries.len(), 1, "should find root tsconfig.json");
    assert!(entries[0].path.ends_with("tsconfig.json"));
    // Negative: should not find non-existent configs
    assert!(
        !entries.iter().any(|e| e.path.contains("tsconfig.app.json")),
        "should not find configs that don't exist"
    );
}

#[test]
fn discover_finds_variant_tsconfigs() {
    let tmp = tempfile::TempDir::new().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    std::fs::write(workspace.join("tsconfig.json"), "{}").unwrap();
    std::fs::write(workspace.join("tsconfig.app.json"), "{}").unwrap();
    std::fs::write(workspace.join("tsconfig.node.json"), "{}").unwrap();

    let entries = discover_tsconfigs(&workspace);
    assert_eq!(entries.len(), 3, "should find root + variant configs");
}

#[test]
fn discover_skips_node_modules() {
    let tmp = tempfile::TempDir::new().unwrap();
    let workspace = tmp.path().join("workspace");
    let nm = workspace.join("node_modules").join("some-pkg");
    std::fs::create_dir_all(&nm).unwrap();
    std::fs::write(nm.join("tsconfig.json"), "{}").unwrap();
    std::fs::write(workspace.join("tsconfig.json"), "{}").unwrap();

    let entries = discover_tsconfigs(&workspace);
    assert_eq!(entries.len(), 1, "should skip node_modules tsconfig");
    assert!(
        !entries.iter().any(|e| e.path.contains("node_modules")),
        "no node_modules paths in results"
    );
}

#[test]
fn discover_nested_packages() {
    let tmp = tempfile::TempDir::new().unwrap();
    let workspace = tmp.path().join("workspace");
    let pkg = workspace.join("packages").join("ui");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(workspace.join("tsconfig.json"), "{}").unwrap();
    std::fs::write(pkg.join("tsconfig.json"), "{}").unwrap();

    let entries = discover_tsconfigs(&workspace);
    assert_eq!(entries.len(), 2, "should find root + nested configs");
}

#[test]
fn discover_empty_dir() {
    let tmp = tempfile::TempDir::new().unwrap();
    let workspace = tmp.path().join("workspace");
    std::fs::create_dir_all(&workspace).unwrap();
    let entries = discover_tsconfigs(&workspace);
    assert!(
        entries.is_empty(),
        "should return empty for dir without tsconfigs"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// parse_tsconfig_json / load_compiler_options
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn parse_base_url() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("tsconfig.json"),
        r#"{ "compilerOptions": { "baseUrl": "." } }"#,
    )
    .unwrap();

    let tsconfig_path = tmp
        .path()
        .join("tsconfig.json")
        .to_string_lossy()
        .replace('\\', "/");
    let opts = load_compiler_options(&ws, &tsconfig_path);
    assert!(opts.base_url.is_some(), "should extract baseUrl");
    let base = opts.base_url.unwrap();
    // baseUrl should be resolved to absolute path
    assert!(!base.is_empty());
    // Negative: no paths should be extracted
    assert!(opts.paths.is_empty(), "no paths in this config");
}

#[test]
fn parse_paths() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("tsconfig.json"),
        r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@/*": ["./src/*"] } } }"#,
    )
    .unwrap();

    let tsconfig_path = tmp
        .path()
        .join("tsconfig.json")
        .to_string_lossy()
        .replace('\\', "/");
    let opts = load_compiler_options(&ws, &tsconfig_path);
    assert_eq!(opts.paths.len(), 1, "should extract one path mapping");
    assert_eq!(opts.paths[0].0, "@/*");
    assert_eq!(opts.paths[0].1.len(), 1);
    assert!(
        opts.paths[0].1[0].contains("src"),
        "path target should contain src"
    );
}

#[test]
fn parse_extends_inherits_options() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("tsconfig.base.json"),
        r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@/*": ["./src/*"] } } }"#,
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("tsconfig.json"),
        r#"{ "extends": "./tsconfig.base.json" }"#,
    )
    .unwrap();

    let tsconfig_path = tmp
        .path()
        .join("tsconfig.json")
        .to_string_lossy()
        .replace('\\', "/");
    let opts = load_compiler_options(&ws, &tsconfig_path);
    assert!(
        opts.base_url.is_some(),
        "should inherit baseUrl from extends"
    );
    assert_eq!(opts.paths.len(), 1, "should inherit paths from extends");
}

#[test]
fn parse_extends_override() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("tsconfig.base.json"),
        r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@/*": ["./lib/*"] } } }"#,
    )
    .unwrap();
    std::fs::write(
        tmp.path().join("tsconfig.json"),
        r#"{ "extends": "./tsconfig.base.json", "compilerOptions": { "paths": { "@/*": ["./src/*"] } } }"#,
    )
    .unwrap();

    let tsconfig_path = tmp
        .path()
        .join("tsconfig.json")
        .to_string_lossy()
        .replace('\\', "/");
    let opts = load_compiler_options(&ws, &tsconfig_path);
    assert_eq!(opts.paths.len(), 1);
    // Should use the overridden value, not the base
    assert!(
        opts.paths[0].1[0].contains("src"),
        "should override paths from base, got: {}",
        opts.paths[0].1[0]
    );
    assert!(
        !opts.paths[0].1[0].contains("lib"),
        "should NOT contain base path target"
    );
}

#[test]
fn parse_nonexistent_file() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let opts = load_compiler_options(&ws, "/nonexistent/tsconfig.json");
    assert!(opts.base_url.is_none());
    assert!(opts.paths.is_empty());
}

// ═══════════════════════════════════════════════════════════════════════════
// load_project_membership
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn membership_match_all_when_no_filters() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("tsconfig.json"),
        r#"{ "compilerOptions": {} }"#,
    )
    .unwrap();

    let tsconfig_path = tmp
        .path()
        .join("tsconfig.json")
        .to_string_lossy()
        .replace('\\', "/");
    let membership = load_project_membership(&ws, &tsconfig_path);
    assert!(
        matches!(membership, ProjectMembership::MatchAll),
        "should be MatchAll when no files/include/exclude"
    );
}

#[test]
fn membership_with_include() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("tsconfig.json"),
        r#"{ "include": ["src/**/*"] }"#,
    )
    .unwrap();

    let tsconfig_path = tmp
        .path()
        .join("tsconfig.json")
        .to_string_lossy()
        .replace('\\', "/");
    let membership = load_project_membership(&ws, &tsconfig_path);
    match membership {
        ProjectMembership::IncludeExclude { include, .. } => {
            assert!(!include.is_empty(), "should have include patterns");
            assert!(
                include[0].contains("src"),
                "include pattern should contain src"
            );
        }
        ProjectMembership::MatchAll => panic!("should be IncludeExclude, not MatchAll"),
    }
}

#[test]
fn membership_with_exclude() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("tsconfig.json"),
        r#"{ "exclude": ["node_modules"] }"#,
    )
    .unwrap();

    let tsconfig_path = tmp
        .path()
        .join("tsconfig.json")
        .to_string_lossy()
        .replace('\\', "/");
    let membership = load_project_membership(&ws, &tsconfig_path);
    match membership {
        ProjectMembership::IncludeExclude { exclude, .. } => {
            assert!(!exclude.is_empty(), "should have exclude patterns");
        }
        ProjectMembership::MatchAll => panic!("should be IncludeExclude"),
    }
}

#[test]
fn exclude_only_carries_the_default_include() {
    // FIX 1: a tsconfig with ONLY `exclude` (no `files`, no `include`) must keep
    // TypeScript's implicit default `**/*` include MINUS the excludes. The
    // producer (`load_project_membership_inner`) models the default include
    // EXPLICITLY: when neither `files` nor `include` is present, the produced
    // membership carries the default `{dir}/**/*` include glob.
    //
    // DISCRIMINATING: before FIX 1 an exclude-only config produced
    // `IncludeExclude { include: [], .. }` (empty include) ⇒ it owned NOTHING;
    // this asserts a NON-EMPTY default include is synthesized.
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("tsconfig.json"),
        r#"{ "exclude": ["dist"] }"#,
    )
    .unwrap();

    let tsconfig_path = tmp
        .path()
        .join("tsconfig.json")
        .to_string_lossy()
        .replace('\\', "/");
    let membership = load_project_membership(&ws, &tsconfig_path);
    match membership {
        ProjectMembership::IncludeExclude {
            files,
            include,
            exclude,
        } => {
            assert!(
                files.is_empty(),
                "an exclude-only config declares no `files`"
            );
            assert!(
                !include.is_empty(),
                "an exclude-only config must carry the implicit default `**/*` include \
                 (was empty before the fix ⇒ owned nothing)"
            );
            assert!(
                include.iter().any(|g| g.ends_with("/**/*")),
                "the synthesized default include must be the `**/*` glob, got {include:?}"
            );
            assert!(!exclude.is_empty(), "the explicit exclude is preserved");
        }
        ProjectMembership::MatchAll => panic!("should be IncludeExclude (has an exclude key)"),
    }
}

/// A package leaf that only adds `compilerOptions.paths` and `extends` an
/// exclude-only monorepo base must synthesize its default include under the
/// **package** directory — not inherit a monorepo-wide `**/*` baked at the
/// base's frame. TypeScript's implicit default include is a property of the
/// FINAL config file's directory, never of an `extends` ancestor's; the old
/// mid-chain synthesis made every paths-only package leaf claim the whole
/// repo and win `@/*` path resolution with the wrong (repo-root) package
/// root.
#[test]
fn package_leaf_extending_exclude_only_base_gets_package_local_default_include() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{ "exclude": ["dist", "node_modules"] }"#,
    )
    .unwrap();
    let pkg = root.join("packages/icons");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(
        pkg.join("tsconfig.json"),
        r#"{
          "extends": "../../tsconfig.json",
          "compilerOptions": {
            "paths": { "@/*": ["./src/*"] }
          }
        }"#,
    )
    .unwrap();

    let leaf = pkg
        .join("tsconfig.json")
        .to_string_lossy()
        .replace('\\', "/");
    // Compare against the package root in the workspace's own canonical form
    // (drive-case + separators), the same coordinate system the synthesized
    // include entries carry, so `starts_with` holds on every platform — a raw
    // `tmp.path()` keeps the OS drive-letter casing (`C:`) while resolver output
    // is lowercase-drive (`c:`), which byte-differs only on Windows.
    let pkg_root = verter_span::path::canonicalize_path(&pkg.to_string_lossy()).replace('\\', "/");
    let membership = load_project_membership(&ws, &leaf);
    match membership {
        ProjectMembership::IncludeExclude {
            include, exclude, ..
        } => {
            assert!(
                !include.is_empty(),
                "paths-only leaf must still get a default include"
            );
            // Every include path is under the package directory (not monorepo root).
            for g in &include {
                assert!(
                    g.starts_with(&pkg_root),
                    "include entry {g} must be under package root {pkg_root}, \
                     not the extends ancestor's directory"
                );
            }
            // The base's excludes still subtract (declared at the base frame,
            // resolved against the base's directory).
            assert!(
                !exclude.is_empty(),
                "the inherited exclude entries are preserved"
            );
        }
        ProjectMembership::MatchAll => {
            panic!("paths-only leaf inheriting exclude should be IncludeExclude, not MatchAll")
        }
    }
}

/// Deliberate-decision pin: a chain that declares NO membership key anywhere
/// (`files`/`include`/`exclude` all absent — here a paths-only leaf extending
/// a compilerOptions-only base) stays `MatchAll`. The leaf default-include
/// synthesis is keyed on membership INTENT (an exclude declared somewhere in
/// the chain), so moving it to the leaf frame must not flip the key-less
/// chain out of `MatchAll` (downstream, `membership_to_spec` already scopes
/// `MatchAll` to the project root with the TS default excludes).
#[test]
fn key_less_extends_chain_stays_match_all() {
    let membership = membership_from_extends(
        r#"{ "compilerOptions": { "strict": true } }"#,
        r#"{ "extends": "./base.json", "compilerOptions": { "paths": { "@/*": ["./src/*"] } } }"#,
    );
    assert!(
        matches!(membership, ProjectMembership::MatchAll),
        "a chain with no files/include/exclude anywhere must stay MatchAll, got {membership:?}"
    );
}

/// Deliberate-decision pin: a missing/unreadable tsconfig keeps the blanket
/// `MatchAll` fallback (nothing declared anywhere — same contract as before
/// the leaf-scoped default-include move; `membership_to_spec` scopes it to
/// the project root downstream).
#[test]
fn missing_config_membership_stays_match_all() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let membership = load_project_membership(&ws, "/nonexistent/tsconfig.json");
    assert!(
        matches!(membership, ProjectMembership::MatchAll),
        "a missing config must stay MatchAll, got {membership:?}"
    );
}

#[test]
fn explicit_empty_files_does_not_synthesize_default_include() {
    // FIX 1 distinction: an EXPLICIT `"files": []` (solution-style, owns
    // nothing but its references) must stay DISTINCT from "no files key at all".
    // `has_files` is true here, so NO default include is synthesized — the
    // include stays empty (owns nothing but references).
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("tsconfig.json"), r#"{ "files": [] }"#).unwrap();

    let tsconfig_path = tmp
        .path()
        .join("tsconfig.json")
        .to_string_lossy()
        .replace('\\', "/");
    let membership = load_project_membership(&ws, &tsconfig_path);
    match membership {
        ProjectMembership::IncludeExclude { files, include, .. } => {
            assert!(files.is_empty(), "explicit `files: []` is empty");
            assert!(
                include.is_empty(),
                "an explicit `files: []` must NOT synthesize a default include \
                 (it owns nothing but its references), got {include:?}"
            );
        }
        ProjectMembership::MatchAll => {
            panic!("an explicit `files` key must produce IncludeExclude, not MatchAll")
        }
    }
}

/// Write `base.json` + `tsconfig.json` (which `extends` it) into a temp dir and
/// return the produced membership for the child. Mirrors the single-file temp
/// pattern above but exercises the `extends` inheritance path.
fn membership_from_extends(base_body: &str, child_body: &str) -> ProjectMembership {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("base.json"), base_body).unwrap();
    std::fs::write(tmp.path().join("tsconfig.json"), child_body).unwrap();

    let tsconfig_path = tmp
        .path()
        .join("tsconfig.json")
        .to_string_lossy()
        .replace('\\', "/");
    load_project_membership(&ws, &tsconfig_path)
}

#[test]
fn inherited_explicit_empty_files_does_not_synthesize_default_include() {
    // An `extends` base that declares an EXPLICIT `"files": []` (solution-style,
    // owns nothing but its references) must keep that distinction across
    // inheritance: the child (which adds only `exclude`) must NOT synthesize a
    // default `**/*` include and must own NOTHING but references.
    //
    // DISCRIMINATING: the inner recursion inherits only the files/include/exclude
    // VECTORS, not whether an ancestor DECLARED `files`/`include`. A base with no
    // membership keys inherits as `MatchAll` ⇒ `(empty, empty, empty)`, which is
    // byte-indistinguishable from a base that declared `files: []`. So before the
    // fix this WRONGLY synthesized `{dir}/**/*` and owned `src/Foo.vue`.
    let membership = membership_from_extends(
        r#"{ "files": [] }"#,
        r#"{ "extends": "./base.json", "exclude": ["dist"] }"#,
    );
    match membership {
        ProjectMembership::IncludeExclude {
            files,
            include,
            exclude,
        } => {
            assert!(files.is_empty(), "inherited explicit `files: []` is empty");
            assert!(
                include.is_empty(),
                "an inherited explicit `files: []` must NOT synthesize a default include \
                 (it owns nothing but its references), got {include:?}"
            );
            assert!(
                !exclude.is_empty(),
                "the child's explicit exclude is preserved"
            );
        }
        ProjectMembership::MatchAll => {
            panic!("an inherited explicit `files` key must produce IncludeExclude, not MatchAll")
        }
    }
}

#[test]
fn inherited_explicit_empty_include_does_not_synthesize_default_include() {
    // Sibling of the above for an inherited EXPLICIT `"include": []`. Same root
    // cause, same fix: inherited declared-but-empty `include` must suppress the
    // default-include synthesis.
    let membership = membership_from_extends(
        r#"{ "include": [] }"#,
        r#"{ "extends": "./base.json", "exclude": ["dist"] }"#,
    );
    match membership {
        ProjectMembership::IncludeExclude {
            files,
            include,
            exclude,
        } => {
            assert!(files.is_empty(), "no files key anywhere in the chain");
            assert!(
                include.is_empty(),
                "an inherited explicit `include: []` must NOT synthesize a default include \
                 (it owns nothing but its references), got {include:?}"
            );
            assert!(
                !exclude.is_empty(),
                "the child's explicit exclude is preserved"
            );
        }
        ProjectMembership::MatchAll => {
            panic!("an inherited explicit `include` key must produce IncludeExclude, not MatchAll")
        }
    }
}

#[test]
fn inherited_real_include_still_owns_default_via_inheritance() {
    // Regression guard for the fix: an `extends` base that declares a REAL
    // (non-empty) `include` must still propagate that include to the child — the
    // child does NOT synthesize a default include (the inherited include is the
    // effective set), and the inherited include is what owns files.
    let membership = membership_from_extends(
        r#"{ "include": ["src"] }"#,
        r#"{ "extends": "./base.json", "exclude": ["dist"] }"#,
    );
    match membership {
        ProjectMembership::IncludeExclude {
            files,
            include,
            exclude,
        } => {
            assert!(files.is_empty(), "no files key anywhere in the chain");
            assert_eq!(
                include.len(),
                1,
                "the inherited REAL `include` (`src`) is preserved, not replaced by a \
                 synthesized default, got {include:?}"
            );
            assert!(
                include[0].ends_with("/src/**/*"),
                "the inherited include points at the base's `src` (the real declared \
                 include, NOT the bare-`**/*` synthesized default), got {include:?}"
            );
            assert!(
                !exclude.is_empty(),
                "the child's explicit exclude is preserved"
            );
        }
        ProjectMembership::MatchAll => {
            panic!("should be IncludeExclude (has an inherited include)")
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// load_project_references
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn references_from_solution_style() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    let app_dir = tmp.path().join("app");
    std::fs::create_dir_all(&app_dir).unwrap();
    std::fs::write(app_dir.join("tsconfig.json"), "{}").unwrap();

    std::fs::write(
        tmp.path().join("tsconfig.json"),
        r#"{ "references": [{ "path": "./app" }] }"#,
    )
    .unwrap();

    let tsconfig_path = tmp
        .path()
        .join("tsconfig.json")
        .to_string_lossy()
        .replace('\\', "/");
    let refs = load_project_references(&ws, &tsconfig_path);
    assert_eq!(refs.len(), 1, "should find 1 reference");
    assert!(
        refs[0].contains("app") && refs[0].ends_with("tsconfig.json"),
        "reference should point to app/tsconfig.json, got: {}",
        refs[0]
    );
}

#[test]
fn references_empty_when_none() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("tsconfig.json"),
        r#"{ "compilerOptions": {} }"#,
    )
    .unwrap();

    let tsconfig_path = tmp
        .path()
        .join("tsconfig.json")
        .to_string_lossy()
        .replace('\\', "/");
    let refs = load_project_references(&ws, &tsconfig_path);
    assert!(refs.is_empty(), "should be empty when no references");
}

// ═══════════════════════════════════════════════════════════════════════════
// normalize_path_buf
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn normalize_path_collapses_dots() {
    let result = normalize_path_buf(Path::new("/project/./src/../lib"));
    assert_eq!(result.replace('\\', "/"), "/project/lib");
}

// ═══════════════════════════════════════════════════════════════════════════
// resolve_tsconfig_extends
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn resolve_relative_extends() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("tsconfig.base.json"), "{}").unwrap();

    let tsconfig_dir = tmp.path().to_string_lossy().replace('\\', "/");
    let resolved = resolve_tsconfig_extends(&ws, &tsconfig_dir, "./tsconfig.base.json");
    assert!(resolved.is_some(), "should resolve relative extends");
}

#[test]
fn resolve_extends_adds_json_extension() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    // with_extension("json") replaces the last extension, so
    // `./base` becomes `./base.json` (which is what we want to find)
    std::fs::write(tmp.path().join("base.json"), "{}").unwrap();

    let tsconfig_dir = tmp.path().to_string_lossy().replace('\\', "/");
    let resolved = resolve_tsconfig_extends(&ws, &tsconfig_dir, "./base");
    assert!(resolved.is_some(), "should try .json extension");
    assert!(
        resolved.unwrap().ends_with("base.json"),
        "should resolve to base.json"
    );
}

#[test]
fn resolve_extends_nonexistent() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    let tsconfig_dir = tmp.path().to_string_lossy().replace('\\', "/");
    let resolved = resolve_tsconfig_extends(&ws, &tsconfig_dir, "./nonexistent");
    assert!(
        resolved.is_none(),
        "should return None for nonexistent extends"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// raw_paths_json inheritance
// ═══════════════════════════════════════════════════════════════════════════

/// Child inherits baseUrl from base, overrides paths → must use base's baseUrl.
#[test]
fn raw_paths_json_inherits_base_url_when_child_overrides_paths() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();

    // Base: defines baseUrl
    std::fs::write(
        tmp.path().join("tsconfig.base.json"),
        r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@old/*": ["old/*"] } } }"#,
    )
    .unwrap();

    // Child: extends base, overrides paths but NOT baseUrl
    std::fs::write(
        tmp.path().join("tsconfig.json"),
        r#"{ "extends": "./tsconfig.base.json", "compilerOptions": { "paths": { "@/*": ["src/*"] } } }"#,
    )
    .unwrap();

    let tsconfig_path = tmp
        .path()
        .join("tsconfig.json")
        .to_string_lossy()
        .replace('\\', "/");
    let result = raw_paths_json(&ws, &tsconfig_path);
    let (base_url, paths) = result.expect("should find paths");

    // baseUrl should come from the base config (resolved to its directory)
    let expected_base = crate::resolver::normalize_canonical_id(&tmp.path().to_string_lossy());
    assert_eq!(
        base_url, expected_base,
        "baseUrl should be inherited from base config, not default to child dir"
    );

    // paths should be the child's override
    let paths_obj = paths.as_object().expect("paths should be an object");
    assert!(
        paths_obj.contains_key("@/*"),
        "should have child's @/* path"
    );
    assert!(
        !paths_obj.contains_key("@old/*"),
        "should NOT have base's @old/* path (child overrides entirely)"
    );
}

/// Child overrides baseUrl but inherits paths → must use child's baseUrl.
#[test]
fn raw_paths_json_child_base_url_overrides_inherited_paths() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    let sub = tmp.path().join("packages/app");
    std::fs::create_dir_all(&sub).unwrap();

    // Base at root: defines paths
    std::fs::write(
        tmp.path().join("tsconfig.base.json"),
        r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@/*": ["src/*"] } } }"#,
    )
    .unwrap();

    // Child in packages/app: overrides baseUrl, inherits paths
    std::fs::write(
        sub.join("tsconfig.json"),
        r#"{ "extends": "../../tsconfig.base.json", "compilerOptions": { "baseUrl": "." } }"#,
    )
    .unwrap();

    let tsconfig_path = sub
        .join("tsconfig.json")
        .to_string_lossy()
        .replace('\\', "/");
    let result = raw_paths_json(&ws, &tsconfig_path);
    let (base_url, paths) = result.expect("should find paths");

    // baseUrl should be the child's override (packages/app/)
    let expected_base = crate::resolver::normalize_canonical_id(&sub.to_string_lossy());
    assert_eq!(
        base_url, expected_base,
        "baseUrl should be child's override, not base's"
    );

    // paths should be inherited from base
    let paths_obj = paths.as_object().expect("paths should be an object");
    assert!(
        paths_obj.contains_key("@/*"),
        "should have inherited @/* path from base"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// discover_tsconfigs — descent pruning
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn discover_tsconfigs_skips_node_modules_and_dot_dirs() {
    // Anchors the prune-at-descent contract that replaced the glob walk.
    //
    // Pre-fix the function used `glob::glob("**/tsconfig.json")` which
    // descended into every directory before post-filtering. Against any
    // real PNPM-managed `node_modules` (where each `.pnpm/<pkg>/node_modules`
    // contains symlinks back into `.pnpm/`) the walk fanned out
    // exponentially and never terminated.
    //
    // Post-fix the function uses `walkdir::WalkDir` with
    // `follow_links(false)` and a `filter_entry` that prunes descent
    // into `node_modules` and any directory whose name starts with
    // `.`. This test pins those two prunes by placing one tsconfig.json
    // inside each forbidden directory and asserting they are absent
    // from the discovered set, while the user-source tsconfigs are
    // present.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    let touch = |rel: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{}").unwrap();
    };

    // User-source: must be discovered.
    touch("tsconfig.json");
    touch("src/tsconfig.app.json");
    touch("packages/lib/tsconfig.json");

    // Forbidden subtrees: must be skipped.
    touch("node_modules/foo/tsconfig.json");
    touch("node_modules/.pnpm/bar/node_modules/bar/tsconfig.json");
    touch(".nuxt/tsconfig.json");
    touch(".git/tsconfig.json");
    touch(".pnpm-store/tsconfig.json");

    // Nested user-source one level inside a non-forbidden dir.
    touch("apps/web/tsconfig.build.json");

    let entries = super::discover_tsconfigs(root);
    let mut paths: Vec<String> = entries.into_iter().map(|e| e.path).collect();
    paths.sort();

    let normalized_root = root.to_string_lossy().replace('\\', "/");
    let expected = vec![
        format!("{normalized_root}/apps/web/tsconfig.build.json"),
        format!("{normalized_root}/packages/lib/tsconfig.json"),
        format!("{normalized_root}/src/tsconfig.app.json"),
        format!("{normalized_root}/tsconfig.json"),
    ];
    let mut expected = expected;
    expected.sort();

    assert_eq!(
        paths, expected,
        "discover_tsconfigs must skip node_modules + dot-dirs at descent",
    );
}

#[test]
fn discover_tsconfigs_matches_tsconfig_and_jsconfig_named_files() {
    // Pin the filename predicate: `tsconfig.json`, `tsconfig.<suffix>.json`,
    // and the JavaScript project config `jsconfig.json` are matched; anything
    // else is not. `jsconfig.json` is the configured-project authority for
    // JS-only trees (tsserver/tsgo honor it natively); a carrier under a
    // jsconfig-only directory must resolve a configured owner (D7).
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let touch = |rel: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{}").unwrap();
    };
    touch("tsconfig.json");
    touch("tsconfig.app.json");
    touch("tsconfig.node.test.json");
    touch("js-only/jsconfig.json");
    // Negative: similar names that must NOT match.
    touch("jsconfig.app.json"); // no suffixed jsconfig variants exist
    touch("tsconfig.txt");
    touch("a-tsconfig.json"); // filename does not start with `tsconfig.`
    touch("tsconfigjson"); // missing dot before extension
    touch("subdir/tsconfig.json");

    let entries = super::discover_tsconfigs(root);
    let mut paths: Vec<String> = entries.iter().map(|e| e.path.clone()).collect();
    paths.sort();
    let file_name = |path: &str| {
        std::path::Path::new(path)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    };
    assert!(
        paths.iter().any(|p| file_name(p) == "jsconfig.json"),
        "jsconfig.json must be discovered as a configured project: {paths:?}",
    );
    assert!(
        !paths.iter().any(|p| file_name(p) == "jsconfig.app.json"),
        "suffixed jsconfig variants must NOT match: {paths:?}",
    );
    let mut names: Vec<String> = paths.iter().map(|p| file_name(p)).collect();
    names.sort();
    names.dedup();
    let expected: Vec<String> = vec![
        "jsconfig.json".into(),
        "tsconfig.app.json".into(),
        "tsconfig.json".into(),
        "tsconfig.node.test.json".into(),
    ];
    assert_eq!(
        names, expected,
        "must match tsconfig[.suffix].json and jsconfig.json files"
    );
    assert_eq!(
        entries
            .iter()
            .filter(|e| e.path.contains("tsconfig.json"))
            .count(),
        2, // root/tsconfig.json + subdir/tsconfig.json
        "should find both root and nested user-source tsconfig.json",
    );
}

#[test]
fn discover_tsconfigs_prefers_tsconfig_json_over_jsconfig_json_in_the_same_directory() {
    // TypeScript ignores a `jsconfig.json` sitting next to a `tsconfig.json`;
    // discovering both would make every file in the directory multiply-owned
    // (Ambiguous) and fail every carrier feature closed.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::write(root.join("tsconfig.json"), "{}").unwrap();
    std::fs::write(root.join("jsconfig.json"), "{}").unwrap();
    std::fs::create_dir_all(root.join("js-only")).unwrap();
    std::fs::write(root.join("js-only").join("jsconfig.json"), "{}").unwrap();

    let entries = super::discover_tsconfigs(root);
    let mut paths: Vec<String> = entries.iter().map(|e| e.path.clone()).collect();
    paths.sort();
    assert_eq!(
        paths.len(),
        2,
        "same-dir jsconfig must be suppressed, standalone jsconfig kept: {paths:?}"
    );
    assert!(paths
        .iter()
        .any(|p| p.ends_with("/tsconfig.json") || p.ends_with("\\tsconfig.json")));
    assert!(paths.iter().any(|p| p.contains("js-only")));
    assert!(
        !paths.iter().any(
            |p| (p.ends_with("/jsconfig.json") || p.ends_with("\\jsconfig.json"))
                && !p.contains("js-only")
        ),
        "the jsconfig next to a tsconfig.json must not be discovered: {paths:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// has_configured_ts_project_anywhere — the bounded owned-tsgo spawn precondition
// ═══════════════════════════════════════════════════════════════════════════

/// A root-only owned-startup gate — a configured project exists ONLY IFF
/// `<root>/tsconfig.json` is a file. Replicated locally as the DISCRIMINATOR baseline:
/// the packages-only monorepo the bounded precondition must accept is EXACTLY the
/// layout a root-only gate rejects, so asserting both on the same fixture proves the
/// precondition is not root-only.
fn root_only_gate(root: &std::path::Path) -> bool {
    root.join("tsconfig.json").is_file()
}

/// DISCRIMINATING: a mainstream monorepo whose configs live only at
/// `packages/*/tsconfig.json` (NO root `tsconfig.json`) HAS a configured project,
/// so the bounded precondition accepts it — while the OLD root-only gate rejected
/// it (the bug this closes). The two assertions on the SAME fixture prove the new
/// behaviour is not root-only behaviour.
#[test]
fn has_configured_ts_project_anywhere_accepts_packages_only_monorepo() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let touch = |rel: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{}").unwrap();
    };
    // A packages-only monorepo: configs live under packages/*, NEVER at the root.
    touch("packages/app/tsconfig.json");
    touch("packages/lib/tsconfig.json");
    // A README at the root so the root dir is non-empty but carries no tsconfig.
    touch("README.md");

    // The OLD root-only gate REJECTS this layout (no `<root>/tsconfig.json`) — the
    // exact bug: a valid monorepo yields NO owned tsgo provider.
    assert!(
        !root_only_gate(root),
        "sanity: the root-only gate rejects a packages-only monorepo (the bug)"
    );
    // The bounded precondition ACCEPTS it — at least one configured project exists.
    assert!(
        super::has_configured_ts_project_anywhere(root),
        "a packages/*/tsconfig.json-only monorepo HAS a configured project, so the \
         owned-tsgo spawn precondition must accept it"
    );
}

/// A workspace with NO `tsconfig.json` anywhere has NO configured project — the
/// precondition returns false (owned tsgo fails closed by NOT spawning, never a
/// config-less inferred project).
#[test]
fn has_configured_ts_project_anywhere_rejects_workspace_with_no_tsconfig() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let touch = |rel: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "x").unwrap();
    };
    touch("src/index.ts");
    touch("packages/app/src/main.ts");
    touch("package.json");

    assert!(
        !super::has_configured_ts_project_anywhere(root),
        "a workspace with no tsconfig anywhere has no configured project"
    );
    // A root-only gate agrees here (no root tsconfig) — this case is not the discriminator.
    assert!(!root_only_gate(root));
}

/// A JS-only workspace configured by `jsconfig.json` (the JavaScript project
/// config tsserver/tsgo honor natively) HAS a configured project: owned tsgo
/// must spawn and bind its carriers (D7 — a jsconfig-only tree is not an
/// inferred project).
#[test]
fn has_configured_ts_project_anywhere_accepts_jsconfig_only_workspace() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let touch = |rel: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{}").unwrap();
    };
    touch("jsconfig.json");
    touch("src/main.js");

    assert!(
        super::has_configured_ts_project_anywhere(root),
        "a jsconfig.json-only workspace HAS a configured JS project"
    );
}

/// The precondition prunes `node_modules` + `.git` + framework-GENERATED dot-dirs
/// (like `.nuxt`) at descent: a workspace whose ONLY tsconfig lives inside
/// `node_modules` (a package-manager artifact) or a generated `.nuxt` build dir has NO
/// authored configured project, so it is rejected. The prune is NARROW — it does NOT
/// exclude a bare authored package name (see
/// `has_configured_ts_project_anywhere_accepts_bare_build_output_named_package_dirs`)
/// nor an authored dot config dir (see
/// `has_configured_ts_project_anywhere_accepts_authored_dot_dir_config`).
#[test]
fn has_configured_ts_project_anywhere_prunes_node_modules_git_and_generated_dot_dirs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let touch = |rel: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{}").unwrap();
    };
    // The ONLY tsconfigs live in pruned subtrees — none is an authored project.
    touch("node_modules/some-dep/tsconfig.json");
    touch("node_modules/.pnpm/x/node_modules/x/tsconfig.json");
    touch(".git/tsconfig.json");
    touch(".nuxt/tsconfig.json");
    touch("src/main.ts");

    assert!(
        !super::has_configured_ts_project_anywhere(root),
        "a tsconfig only inside node_modules / .git / a generated dot-dir (.nuxt) is not \
         an authored configured project — the precondition prunes those subtrees and \
         rejects the workspace"
    );

    // But a real authored config UNDER a non-pruned dir (alongside the pruned ones)
    // IS found — proving the prune does not over-reject.
    touch("apps/web/tsconfig.json");
    assert!(
        super::has_configured_ts_project_anywhere(root),
        "an authored packages/apps tsconfig outside node_modules must be found"
    );
}

/// DISCRIMINATING: an AUTHORED config dir whose name starts with `.` but is NOT a
/// framework-GENERATED dir — e.g. `.storybook/tsconfig.json` — IS an authored
/// configured project, so the precondition accepts a workspace whose ONLY config lives
/// there. The OLD all-dot-dirs prune REJECTED it (rejecting a workspace whose only
/// configured project is under a dot-dir — the bug this closes), while a
/// `node_modules`-only tsconfig stays pruned. The two assertions on sibling fixtures
/// prove the prune is narrowed to node_modules + `.git` + generated dot-dirs, NOT
/// every dot-directory.
#[test]
fn has_configured_ts_project_anywhere_accepts_authored_dot_dir_config() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    let touch = |rel: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "{}").unwrap();
    };
    // The ONLY authored config lives under `.storybook` (an authored tooling dir, NOT a
    // framework-generated dot-dir).
    touch(".storybook/tsconfig.json");
    touch("src/main.ts");

    // A root-only gate rejects it (no `<root>/tsconfig.json`), and the OLD all-dot-dirs
    // prune ALSO rejected it — the bug: a `.storybook`-configured workspace yielded NO
    // owned tsgo provider even though it HAS a configured project.
    assert!(
        !root_only_gate(root),
        "sanity: the root-only gate rejects a `.storybook`-only workspace"
    );
    assert!(
        super::has_configured_ts_project_anywhere(root),
        "an authored `.storybook/tsconfig.json` is a configured project — the narrowed \
         prune (node_modules + .git + generated dot-dirs, NOT every dot-dir) must \
         accept it"
    );

    // The DISCRIMINATOR sibling: a workspace whose ONLY tsconfig lives in `node_modules`
    // is still pruned (a package-manager artifact, never an authored project).
    let tmp_nm = tempfile::tempdir().expect("tempdir");
    let root_nm = tmp_nm.path();
    std::fs::create_dir_all(root_nm.join("node_modules/dep")).unwrap();
    std::fs::write(root_nm.join("node_modules/dep/tsconfig.json"), "{}").unwrap();
    assert!(
        !super::has_configured_ts_project_anywhere(root_nm),
        "a `node_modules`-only tsconfig must still be pruned (never an authored project)"
    );
}

/// DISCRIMINATING: a workspace whose ONLY configured project lives in a BARE (non-dot)
/// authored package directory that happens to share a
/// name with a build-output dir — `packages/build/tsconfig.json`, and likewise `out` /
/// `dist` / `target` / `coverage` — IS a configured project, so the precondition MUST
/// accept it. A prune-by-basename blocklist that pruned those bare names false-negatived
/// this exact layout and reintroduced the "OWNED refuses to spawn" bug (a spawn
/// precondition must err toward SPAWNING; the per-query `BoundProject` gate is the real
/// authority). RED against the bare-name blocklist, GREEN after narrowing the prune to
/// `node_modules` + `.git` + generated dot-dirs only.
#[test]
fn has_configured_ts_project_anywhere_accepts_bare_build_output_named_package_dirs() {
    // Every one of these was in the pre-fix prune-by-basename blocklist, yet each is a
    // legitimate authored package name a carrier can bind to.
    for pkg in ["build", "out", "dist", "target", "coverage"] {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let cfg = root.join("packages").join(pkg).join("tsconfig.json");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        std::fs::write(&cfg, "{}").unwrap();
        // A README at the root so the tree is non-empty but carries no root tsconfig.
        std::fs::write(root.join("README.md"), "x").unwrap();

        // No root tsconfig ⇒ a root-only gate rejects it; the narrowed precondition accepts.
        assert!(
            !root_only_gate(root),
            "sanity: no root tsconfig for the `packages/{pkg}` fixture"
        );
        assert!(
            super::has_configured_ts_project_anywhere(root),
            "a bare authored `packages/{pkg}/tsconfig.json` IS a configured project — the \
             prune must NOT exclude an ambiguous authored-source name (it did before the \
             fix, false-refusing OWNED spawn)"
        );
    }
}

/// A classic single-root workspace (`<root>/tsconfig.json`) is still accepted —
/// the precondition is a SUPERSET of a root-only gate, never a regression.
#[test]
fn has_configured_ts_project_anywhere_still_accepts_root_tsconfig() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();
    std::fs::write(root.join("tsconfig.json"), "{}").unwrap();

    assert!(
        root_only_gate(root),
        "sanity: a root-only gate accepts a root tsconfig"
    );
    assert!(
        super::has_configured_ts_project_anywhere(root),
        "a classic single-root workspace is still accepted (a superset of a root-only gate)"
    );
}
