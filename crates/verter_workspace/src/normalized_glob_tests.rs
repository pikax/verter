use super::*;
use crate::canonical_path::CanonicalPath;

// ── Normalization ──

#[test]
fn backslash_to_forward_slash() {
    let g = NormalizedGlob::new("d:\\project\\src\\**\\*.vue");
    assert_eq!(g.as_str(), "d:/project/src/**/*.vue");
    assert!(!g.as_str().contains('\\'));
}

#[cfg(windows)]
#[test]
fn lowercase_drive_letter_windows() {
    let g = NormalizedGlob::new("D:/project/src/**/*.vue");
    assert!(
        g.as_str().starts_with("d:"),
        "drive letter should be lowercase"
    );
}

#[cfg(not(windows))]
#[test]
fn no_case_transform_on_linux() {
    let g = NormalizedGlob::new("/Home/User/**/*.vue");
    assert_eq!(g.as_str(), "/Home/User/**/*.vue");
}

// ── from_root_and_pattern ──

#[test]
fn from_root_and_pattern_joins() {
    let root = CanonicalPath::new("d:/project");
    let g = NormalizedGlob::from_root_and_pattern(&root, "src/**/*.vue");
    assert!(g.as_str().ends_with("/project/src/**/*.vue"));
}

#[test]
fn from_root_and_pattern_strips_leading_slash() {
    let root = CanonicalPath::new("d:/project");
    let g = NormalizedGlob::from_root_and_pattern(&root, "/src/**");
    assert!(g.as_str().ends_with("/project/src/**"));
    // Should NOT have double slash
    assert!(
        !g.as_str().contains("//"),
        "should not have double slash, got: {}",
        g.as_str()
    );
}

// ── Matching ──

#[test]
fn matches_simple_glob() {
    let g = NormalizedGlob::new("d:/project/src/**/*.vue");
    assert!(g.matches(&CanonicalPath::new("d:/project/src/App.vue")));
    assert!(g.matches(&CanonicalPath::new("d:/project/src/components/Button.vue")));
}

#[test]
fn no_match_outside_glob() {
    let g = NormalizedGlob::new("d:/project/src/**/*.vue");
    assert!(!g.matches(&CanonicalPath::new("d:/project/tests/foo.vue")));
    assert!(!g.matches(&CanonicalPath::new("d:/project/src/main.ts")));
}

#[test]
fn matches_star_star_everything() {
    let g = NormalizedGlob::new("d:/project/**/*");
    assert!(g.matches(&CanonicalPath::new("d:/project/src/foo.ts")));
    assert!(g.matches(&CanonicalPath::new("d:/project/deeply/nested/file.vue")));
}

#[test]
fn no_match_node_modules_unless_glob_allows() {
    let g = NormalizedGlob::new("d:/project/src/**/*");
    // node_modules under src would technically match the glob
    assert!(g.matches(&CanonicalPath::new(
        "d:/project/src/node_modules/foo/index.ts"
    )));

    // But a glob for node_modules specifically
    let exclude = NormalizedGlob::new("d:/project/node_modules/**");
    assert!(exclude.matches(&CanonicalPath::new("d:/project/node_modules/vue/index.ts")));
    assert!(!exclude.matches(&CanonicalPath::new("d:/project/src/foo.ts")));
}

#[test]
fn matches_single_file_pattern() {
    // Non-glob pattern (no wildcards) should match exactly
    let g = NormalizedGlob::new("d:/project/src/main.ts");
    assert!(g.matches(&CanonicalPath::new("d:/project/src/main.ts")));
    assert!(!g.matches(&CanonicalPath::new("d:/project/src/main.tsx")));
}

// ── Separator semantics ──

#[test]
fn single_star_does_not_cross_directories() {
    // TypeScript's * does NOT match /  — only ** does
    let g = NormalizedGlob::new("d:/project/*");
    assert!(g.matches(&CanonicalPath::new("d:/project/main.ts")));
    assert!(
        !g.matches(&CanonicalPath::new("d:/project/src/main.ts")),
        "single * should NOT match across directory boundaries"
    );
}

#[test]
fn double_star_crosses_directories() {
    let g = NormalizedGlob::new("d:/project/**/*.ts");
    assert!(g.matches(&CanonicalPath::new("d:/project/main.ts")));
    assert!(g.matches(&CanonicalPath::new("d:/project/src/deep/main.ts")));
}

// ── Invalid glob ──

#[test]
fn invalid_glob_returns_false() {
    // Malformed pattern should not panic — just returns false
    let g = NormalizedGlob::new("d:/project/[invalid");
    assert!(!g.matches(&CanonicalPath::new("d:/project/foo.ts")));
}

// ── Display ──

#[test]
fn display_matches_as_str() {
    let g = NormalizedGlob::new("d:/project/**/*.vue");
    assert_eq!(format!("{}", g), g.as_str());
}
