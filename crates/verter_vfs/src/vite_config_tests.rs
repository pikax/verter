use super::*;
use std::path::PathBuf;

// ── Config discovery ─────────────────────────────────────────────────

#[test]
fn config_discovery_priority_order() {
    let tmp = tempfile::TempDir::new().unwrap();

    // Create multiple config files
    for name in VITE_CONFIG_NAMES {
        std::fs::write(tmp.path().join(name), "export default {}").unwrap();
    }

    // Should find vite.config.ts first (highest priority)
    let found = find_vite_config(tmp.path());
    assert!(found.is_some(), "should find vite config");
    assert!(
        found.unwrap().ends_with("vite.config.ts"),
        "should prefer .ts extension"
    );

    // Remove .ts, should fall back to .js
    std::fs::remove_file(tmp.path().join("vite.config.ts")).unwrap();
    let found = find_vite_config(tmp.path());
    assert!(
        found.unwrap().ends_with("vite.config.js"),
        "should fall back to .js"
    );
}

#[test]
fn config_discovery_not_found() {
    let tmp = tempfile::TempDir::new().unwrap();
    assert!(find_vite_config(tmp.path()).is_none());
}

// ── Alias normalization ──────────────────────────────────────────────

#[test]
fn normalize_bare_alias_gets_slash() {
    let dir = PathBuf::from("/project");
    let (find, _) = normalize_alias_pair("@", "./src", &dir);
    assert_eq!(find, "@/", "bare @ should become @/");
}

#[test]
fn normalize_already_slashed_alias() {
    let dir = PathBuf::from("/project");
    let (find, _) = normalize_alias_pair("@/", "./src", &dir);
    assert_eq!(find, "@/", "already slashed should stay @/");
}

#[test]
fn normalize_relative_replacement_becomes_absolute() {
    let dir = PathBuf::from("/project");
    let (_, replacement) = normalize_alias_pair("@", "./src", &dir);
    assert!(
        replacement.starts_with("/project"),
        "relative replacement should be made absolute, got: {replacement}"
    );
    assert!(
        replacement.contains("src"),
        "should contain src segment, got: {replacement}"
    );
}

#[test]
fn normalize_absolute_replacement_preserved() {
    let dir = PathBuf::from("/project");
    let (_, replacement) = normalize_alias_pair("@", "/absolute/src", &dir);
    assert_eq!(replacement, "/absolute/src", "absolute should be preserved");
}

// ── Static analysis — supported shapes ───────────────────────────────

#[test]
fn static_analysis_simple_object_alias() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();

    std::fs::write(
        tmp.path().join("vite.config.ts"),
        "export default { resolve: { alias: { '@': './src' } } }",
    )
    .unwrap();

    let result = analyze_vite_config(tmp.path());
    match &result {
        ViteConfigAnalysis::Resolved {
            aliases,
            config_path,
            ..
        } => {
            assert_eq!(aliases.len(), 1, "should find 1 alias");
            assert_eq!(aliases[0].0, "@/", "find should be @/");
            assert!(
                aliases[0].1.contains("src"),
                "replacement should contain src"
            );
            assert!(config_path.contains("vite.config.ts"));
        }
        ViteConfigAnalysis::Complex { reason, .. } => {
            panic!("expected Resolved, got Complex: {reason}");
        }
        ViteConfigAnalysis::NotFound => panic!("expected Resolved, got NotFound"),
    }

    // Negative: no complexity trigger
    assert!(
        !matches!(result, ViteConfigAnalysis::Complex { .. }),
        "simple object alias should not be Complex"
    );
}

#[test]
fn static_analysis_define_config_wrapper() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();

    std::fs::write(
        tmp.path().join("vite.config.ts"),
        r#"import { defineConfig } from 'vite'
export default defineConfig({ resolve: { alias: { '@': './src' } } })"#,
    )
    .unwrap();

    match analyze_vite_config(tmp.path()) {
        ViteConfigAnalysis::Resolved { aliases, .. } => {
            assert_eq!(aliases.len(), 1);
            assert_eq!(aliases[0].0, "@/");
        }
        ViteConfigAnalysis::Complex { reason, .. } => {
            panic!("expected Resolved for defineConfig wrapper, got Complex: {reason}");
        }
        ViteConfigAnalysis::NotFound => panic!("expected Resolved"),
    }
}

#[test]
fn static_analysis_const_indirection() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();

    std::fs::write(
        tmp.path().join("vite.config.js"),
        r#"const config = { resolve: { alias: { '@': './src' } } };
export default config;"#,
    )
    .unwrap();

    match analyze_vite_config(tmp.path()) {
        ViteConfigAnalysis::Resolved { aliases, .. } => {
            assert_eq!(aliases.len(), 1);
            assert_eq!(aliases[0].0, "@/");
        }
        ViteConfigAnalysis::Complex { reason, .. } => {
            panic!("expected Resolved for const indirection, got Complex: {reason}");
        }
        ViteConfigAnalysis::NotFound => panic!("expected Resolved"),
    }
}

#[test]
fn static_analysis_template_literal_value() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();

    std::fs::write(
        tmp.path().join("vite.config.js"),
        "export default { resolve: { alias: { '@': `./src` } } }",
    )
    .unwrap();

    match analyze_vite_config(tmp.path()) {
        ViteConfigAnalysis::Resolved { aliases, .. } => {
            assert_eq!(aliases.len(), 1);
            assert_eq!(aliases[0].0, "@/");
        }
        other => panic!("expected Resolved for template literal, got {other:?}"),
    }
}

#[test]
fn static_analysis_array_format() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();
    std::fs::create_dir_all(tmp.path().join("lib")).unwrap();

    std::fs::write(
        tmp.path().join("vite.config.mjs"),
        r#"export default {
  resolve: {
    alias: [
      { find: '@', replacement: './src' },
      { find: '~', replacement: './lib' },
    ]
  }
}"#,
    )
    .unwrap();

    match analyze_vite_config(tmp.path()) {
        ViteConfigAnalysis::Resolved { aliases, .. } => {
            assert_eq!(aliases.len(), 2);
            assert!(aliases.iter().any(|(f, _)| f == "@/"));
            assert!(aliases.iter().any(|(f, _)| f == "~/"));
        }
        ViteConfigAnalysis::Complex { reason, .. } => {
            panic!("expected Resolved for array alias, got Complex: {reason}");
        }
        ViteConfigAnalysis::NotFound => panic!("expected Resolved"),
    }
}

#[test]
fn static_analysis_new_url_import_meta() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();

    std::fs::write(
        tmp.path().join("vite.config.ts"),
        r#"export default {
  resolve: {
    alias: {
      '@': new URL('./src', import.meta.url)
    }
  }
}"#,
    )
    .unwrap();

    match analyze_vite_config(tmp.path()) {
        ViteConfigAnalysis::Resolved { aliases, .. } => {
            assert_eq!(aliases.len(), 1);
            assert_eq!(aliases[0].0, "@/");
            assert!(aliases[0].1.contains("src"), "should resolve to src dir");
        }
        ViteConfigAnalysis::Complex { reason, .. } => {
            panic!("expected Resolved for new URL, got Complex: {reason}");
        }
        ViteConfigAnalysis::NotFound => panic!("expected Resolved"),
    }
}

#[test]
fn static_analysis_file_url_to_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("src")).unwrap();

    std::fs::write(
        tmp.path().join("vite.config.ts"),
        r#"import { fileURLToPath } from 'node:url'
export default {
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url))
    }
  }
}"#,
    )
    .unwrap();

    match analyze_vite_config(tmp.path()) {
        ViteConfigAnalysis::Resolved { aliases, .. } => {
            assert_eq!(aliases.len(), 1);
            assert_eq!(aliases[0].0, "@/");
            assert!(aliases[0].1.contains("src"));
        }
        ViteConfigAnalysis::Complex { reason, .. } => {
            panic!("expected Resolved for fileURLToPath, got Complex: {reason}");
        }
        ViteConfigAnalysis::NotFound => panic!("expected Resolved"),
    }
}

// ── Static analysis — complexity triggers ────────────────────────────

#[test]
fn static_analysis_function_export_is_complex() {
    let tmp = tempfile::TempDir::new().unwrap();

    std::fs::write(
        tmp.path().join("vite.config.ts"),
        r#"import { defineConfig } from 'vite'
export default defineConfig(({ mode }) => ({
  resolve: { alias: { '@': './src' } }
}))"#,
    )
    .unwrap();

    match analyze_vite_config(tmp.path()) {
        ViteConfigAnalysis::Complex { reason, .. } => {
            assert!(
                reason.contains("function") || reason.contains("arrow"),
                "reason should mention function/arrow: {reason}"
            );
        }
        ViteConfigAnalysis::Resolved { .. } => {
            panic!("function export should be Complex");
        }
        ViteConfigAnalysis::NotFound => panic!("should find config"),
    }
}

#[test]
fn static_analysis_process_env_is_complex() {
    let tmp = tempfile::TempDir::new().unwrap();

    std::fs::write(
        tmp.path().join("vite.config.ts"),
        r#"const dir = process.env.SRC_DIR || './src'
export default { resolve: { alias: { '@': dir } } }"#,
    )
    .unwrap();

    match analyze_vite_config(tmp.path()) {
        ViteConfigAnalysis::Complex { reason, .. } => {
            assert!(
                reason.contains("process.env"),
                "reason should mention process.env: {reason}"
            );
        }
        other => panic!("process.env should be Complex, got {other:?}"),
    }
}

#[test]
fn static_analysis_import_meta_env_is_complex() {
    let tmp = tempfile::TempDir::new().unwrap();

    std::fs::write(
        tmp.path().join("vite.config.ts"),
        r#"const isProd = import.meta.env.PROD
export default { resolve: { alias: { '@': './src' } } }"#,
    )
    .unwrap();

    match analyze_vite_config(tmp.path()) {
        ViteConfigAnalysis::Complex { reason, .. } => {
            assert!(reason.contains("import.meta.env"));
        }
        other => panic!("import.meta.env should be Complex, got {other:?}"),
    }
}

#[test]
fn static_analysis_dynamic_import_is_complex() {
    let tmp = tempfile::TempDir::new().unwrap();

    std::fs::write(
        tmp.path().join("vite.config.ts"),
        r#"const plugin = import('./plugin')
export default { resolve: { alias: { '@': './src' } } }"#,
    )
    .unwrap();

    match analyze_vite_config(tmp.path()) {
        ViteConfigAnalysis::Complex { reason, .. } => {
            assert!(
                reason.contains("dynamic import"),
                "reason should mention dynamic import: {reason}"
            );
        }
        other => panic!("dynamic import should be Complex, got {other:?}"),
    }
}

#[test]
fn static_analysis_non_allowlisted_package_is_complex() {
    let tmp = tempfile::TempDir::new().unwrap();

    std::fs::write(
        tmp.path().join("vite.config.ts"),
        r#"import lodash from 'lodash'
export default { resolve: { alias: { '@': './src' } } }"#,
    )
    .unwrap();

    match analyze_vite_config(tmp.path()) {
        ViteConfigAnalysis::Complex { reason, .. } => {
            assert!(
                reason.contains("lodash"),
                "reason should mention the package: {reason}"
            );
        }
        other => panic!("non-allowlisted package should be Complex, got {other:?}"),
    }
}

#[test]
fn static_analysis_computed_key_is_complex() {
    let tmp = tempfile::TempDir::new().unwrap();

    std::fs::write(
        tmp.path().join("vite.config.ts"),
        r#"const key = '@'
export default { resolve: { alias: { [key]: './src' } } }"#,
    )
    .unwrap();

    match analyze_vite_config(tmp.path()) {
        ViteConfigAnalysis::Complex { reason, .. } => {
            assert!(
                reason.contains("computed"),
                "reason should mention computed: {reason}"
            );
        }
        other => panic!("computed key should be Complex, got {other:?}"),
    }
}

#[test]
fn static_analysis_not_found() {
    let tmp = tempfile::TempDir::new().unwrap();

    assert!(matches!(
        analyze_vite_config(tmp.path()),
        ViteConfigAnalysis::NotFound
    ));
}

// ── LKG cache ────────────────────────────────────────────────────────

#[test]
fn lkg_cache_stores_and_retrieves() {
    cache_lkg(
        "/test/lkg_vfs/vite.config.ts",
        &[("@/".to_string(), "/test/src".to_string())],
    );
    let result = get_lkg("/test/lkg_vfs/vite.config.ts");
    assert!(result.is_some());
    assert_eq!(result.unwrap().len(), 1);
}

#[test]
fn lkg_cache_empty_for_unknown() {
    let result = get_lkg("/nonexistent_vfs/path/vite.config.ts");
    assert!(result.is_none());
}

#[test]
fn lkg_or_empty_returns_empty_vec() {
    let result = get_lkg_or_empty("/truly/unknown_vfs/vite.config.ts");
    assert!(result.is_empty());
}

// ── Trusted execution ────────────────────────────────────────────────

#[test]
fn trusted_execution_env_sanitization() {
    let tmp = tempfile::TempDir::new().unwrap();
    std::fs::write(tmp.path().join("vite.config.js"), "export default {}").unwrap();

    // With a non-existent node path, should return None gracefully
    let result = execute_trusted_vite_config(
        &tmp.path().join("vite.config.js"),
        tmp.path(),
        "/nonexistent/node",
    );
    assert!(result.is_none(), "should return None for missing node");
}
