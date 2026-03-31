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
// has_solution_style_tsconfig
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn solution_style_detected() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("tsconfig.json"),
        r#"{ "references": [{ "path": "./app" }] }"#,
    )
    .unwrap();

    let workspace_root = tmp.path().to_string_lossy().replace('\\', "/");
    assert!(
        has_solution_style_tsconfig(&ws, &workspace_root),
        "should detect solution-style tsconfig"
    );
}

#[test]
fn no_solution_style_without_references() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(
        tmp.path().join("tsconfig.json"),
        r#"{ "compilerOptions": {} }"#,
    )
    .unwrap();

    let workspace_root = tmp.path().to_string_lossy().replace('\\', "/");
    assert!(
        !has_solution_style_tsconfig(&ws, &workspace_root),
        "should not detect solution-style without references"
    );
}

#[test]
fn no_solution_style_empty_references() {
    let ws = crate::filesystem::FilesystemWorkspace::new(
        crate::filesystem::FilesystemOptions::default(),
    );
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("tsconfig.json"), r#"{ "references": [] }"#).unwrap();

    let workspace_root = tmp.path().to_string_lossy().replace('\\', "/");
    assert!(
        !has_solution_style_tsconfig(&ws, &workspace_root),
        "should not detect solution-style with empty references"
    );
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
