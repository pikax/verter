use super::*;

// ── Normalization ──

#[test]
fn backslash_to_forward_slash() {
    let p = CanonicalPath::new("d:\\project\\src\\foo.vue");
    assert_eq!(p.as_str(), "d:/project/src/foo.vue");
    assert!(!p.as_str().contains('\\'), "no backslashes should remain");
}

#[test]
fn strip_extended_length_prefix() {
    let p = CanonicalPath::new("\\\\?\\C:\\foo\\bar");
    assert_eq!(p.as_str(), "c:/foo/bar");
    assert!(!p.as_str().contains("?"), "no ? should remain");
}

#[test]
fn strip_unc_prefix() {
    let p = CanonicalPath::new("\\\\?\\UNC\\server\\share\\file.txt");
    assert_eq!(p.as_str(), "//server/share/file.txt");
}

#[test]
fn lowercase_drive_letter_for_windows_style_paths() {
    let p = CanonicalPath::new("D:/Project/src/foo.vue");
    assert_eq!(p.as_str(), "d:/Project/src/foo.vue");
    assert!(
        p.as_str().starts_with("d:"),
        "drive letter should be lowercase"
    );
    // Only the drive letter is lowered, not the rest of the path
    assert!(
        p.as_str().contains("Project"),
        "path components should NOT be lowercased"
    );
}

#[test]
fn already_lowercase_drive_unchanged() {
    let p = CanonicalPath::new("c:/project/foo.ts");
    assert_eq!(p.as_str(), "c:/project/foo.ts");
}

#[cfg(not(windows))]
#[test]
fn no_case_transform_on_linux() {
    let p = CanonicalPath::new("/home/User/Project/foo.vue");
    assert_eq!(
        p.as_str(),
        "/home/User/Project/foo.vue",
        "Linux paths should NOT be case-transformed"
    );
}

#[test]
fn forward_slashes_pass_through() {
    let p = CanonicalPath::new("d:/project/src/foo.vue");
    assert_eq!(p.as_str(), "d:/project/src/foo.vue");
}

// ── Directory boundary matching ──

#[test]
fn starts_with_dir_exact_match() {
    let path = CanonicalPath::new("d:/project");
    let prefix = CanonicalPath::new("d:/project");
    assert!(path.starts_with_dir(&prefix));
}

#[test]
fn starts_with_dir_child() {
    let path = CanonicalPath::new("d:/project/src/foo.vue");
    let prefix = CanonicalPath::new("d:/project");
    assert!(path.starts_with_dir(&prefix));
}

#[test]
fn starts_with_dir_rejects_partial_prefix() {
    let path = CanonicalPath::new("d:/project-extra/foo.vue");
    let prefix = CanonicalPath::new("d:/project");
    assert!(
        !path.starts_with_dir(&prefix),
        "project-extra should NOT match project prefix"
    );
}

#[test]
fn starts_with_dir_different_root() {
    let path = CanonicalPath::new("d:/other/foo.vue");
    let prefix = CanonicalPath::new("d:/project");
    assert!(!path.starts_with_dir(&prefix));
}

// ── Trait impls ──

#[test]
fn display_matches_as_str() {
    let p = CanonicalPath::new("d:/project/foo.vue");
    assert_eq!(format!("{}", p), p.as_str());
}

#[test]
fn from_str_creates_canonical() {
    let p: CanonicalPath = "d:\\project\\foo.vue".into();
    assert_eq!(
        p.as_str(),
        CanonicalPath::new("d:\\project\\foo.vue").as_str()
    );
}

#[test]
fn from_string_creates_canonical() {
    let raw = String::from("d:\\project\\foo.vue");
    let p: CanonicalPath = raw.into();
    assert_eq!(
        p.as_str(),
        CanonicalPath::new("d:\\project\\foo.vue").as_str()
    );
}

#[test]
fn into_string_consumes() {
    let p = CanonicalPath::new("d:/project/foo.vue");
    let s = p.into_string();
    assert!(s.contains("project/foo.vue"));
}

// ── Equality and ordering ──

#[test]
fn equal_after_normalization() {
    let a = CanonicalPath::new("d:\\project\\foo.vue");
    let b = CanonicalPath::new("d:/project/foo.vue");
    assert_eq!(a, b);
}

#[test]
fn hash_consistent_after_normalization() {
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(CanonicalPath::new("d:\\project\\foo.vue"));
    // Same path with forward slashes should be found
    assert!(set.contains(&CanonicalPath::new("d:/project/foo.vue")));
}

// ── Trailing slash stripping ──

#[test]
fn trailing_slash_stripped() {
    let p = CanonicalPath::new("d:/project/");
    assert_eq!(p.as_str(), CanonicalPath::new("d:/project").as_str());
    assert!(
        !p.as_str().ends_with('/'),
        "trailing slash should be stripped"
    );
}

#[test]
fn trailing_slash_preserved_for_root() {
    // Root "/" should keep its slash
    let p = CanonicalPath::new("/");
    assert_eq!(p.as_str(), "/");
}

#[cfg(windows)]
#[test]
fn trailing_slash_preserved_for_drive_root() {
    // "C:/" is a root — keep the trailing slash
    let p = CanonicalPath::new("C:/");
    assert_eq!(p.as_str(), "c:/");
}

#[test]
fn starts_with_dir_works_after_trailing_slash_stripped() {
    // This was the bug: "d:/project/" as prefix would fail starts_with_dir
    let path = CanonicalPath::new("d:/project/src/foo.vue");
    let prefix = CanonicalPath::new("d:/project/");
    assert!(
        path.starts_with_dir(&prefix),
        "trailing slash on prefix should be stripped, making match work"
    );
}

// ── Empty string ──

#[test]
fn empty_string_produces_empty() {
    let p = CanonicalPath::new("");
    assert_eq!(p.as_str(), "");
}

// ── canonicalize_path standalone ──

#[test]
fn canonicalize_path_standalone() {
    let result = canonicalize_path("D:\\project\\src\\foo.vue");
    assert!(result.contains("/project/src/foo.vue"));
    assert!(!result.contains('\\'));
}
