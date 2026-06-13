use super::*;
use std::borrow::Cow;

// ── canonicalize_path: drive lowering + slash + casing preservation ──

#[test]
fn windows_drive_lowered_rest_casing_preserved() {
    let p = canonicalize_path("C:\\Users\\Dev\\App.vue");
    assert_eq!(p, "c:/Users/Dev/App.vue");
    // NEGATIVE: whole-path lowercasing is a bug — must not happen.
    assert_ne!(p, "c:/users/dev/app.vue");
    assert!(!p.contains('\\'), "no backslashes should remain");
}

#[test]
fn already_lowercase_drive_unchanged() {
    assert_eq!(canonicalize_path("c:/project/foo.ts"), "c:/project/foo.ts");
}

// ── UNC / extended-length prefixes ──

#[test]
fn unc_extended_prefix_stripped() {
    assert_eq!(canonicalize_path("\\\\?\\C:\\x"), "c:/x");
}

#[test]
fn unc_server_share_prefix_stripped() {
    // //?/UNC/ must be matched BEFORE //?/
    assert_eq!(canonicalize_path("//?/UNC/server/share"), "//server/share");
}

#[test]
fn unc_drive_prefix_lowered() {
    assert_eq!(canonicalize_path("//?/C:/x"), "c:/x");
}

// ── whole-path case preservation (project_graph bug regression) ──

#[test]
fn non_drive_path_casing_fully_preserved() {
    let p = canonicalize_path("/Users/Foo/Bar.vue");
    assert_eq!(p, "/Users/Foo/Bar.vue");
    // NEGATIVE: the old project_graph normalize_path lowercased the WHOLE path.
    assert_ne!(p, "/users/foo/bar.vue");
}

// ── trailing slash ──

#[test]
fn trailing_slash_stripped_except_roots() {
    assert_eq!(canonicalize_path("/a/b/"), "/a/b");
    assert_eq!(canonicalize_path("/"), "/");
    assert_eq!(canonicalize_path("c:/"), "c:/");
}

#[test]
fn trailing_slash_strip_is_idempotent_over_repeats() {
    // Pre-fix the strip popped only ONE slash, so `/a//` → `/a/` (still
    // strippable) and a second call would change it again — two canonical IDs
    // for one directory. ALL trailing slashes must collapse in a single call.
    assert_eq!(canonicalize_path("/a//"), "/a");
    assert_eq!(canonicalize_path("/a///"), "/a");
    assert_eq!(canonicalize_path("c:/x//"), "c:/x");
    assert_eq!(canonicalize_path("/a/b//"), "/a/b");
    // Idempotence: canonicalizing a canonical value is a fixed point.
    let once = canonicalize_path("/a//");
    assert_eq!(canonicalize_path(&once), once);
    // Roots are still preserved (not over-stripped).
    assert_eq!(canonicalize_path("c:///"), "c:/");
    assert_eq!(canonicalize_path("//"), "/");
}

// ── Cow borrowed fast path ──

#[test]
fn cow_borrowed_when_already_canonical() {
    assert!(matches!(
        canonicalize_path_cow("/users/app.ts"),
        Cow::Borrowed(_)
    ));
    assert!(matches!(canonicalize_path_cow("c:/x/y"), Cow::Borrowed(_)));
}

#[test]
fn cow_owned_when_transform_needed() {
    assert!(matches!(canonicalize_path_cow("C:\\x"), Cow::Owned(_)));
    assert!(matches!(canonicalize_path_cow("/a/b/"), Cow::Owned(_)));
    assert!(matches!(canonicalize_path_cow("C:/x"), Cow::Owned(_)));
}

// ── is_under_dir / starts_with_dir directory-boundary containment ──

#[test]
fn is_under_dir_child_and_exact() {
    assert!(is_under_dir("/a/project/x.ts", "/a/project"));
    assert!(is_under_dir("/a/project", "/a/project"));
}

#[test]
fn is_under_dir_rejects_sibling_prefix() {
    // /a/project-extra must NOT match prefix /a/project (sibling-prefix bug).
    assert!(!is_under_dir("/a/project-extra/x", "/a/project"));
}

#[test]
fn is_under_dir_is_case_preserving() {
    // Containment is case-sensitive: /a/App.vue is NOT under /a/app.
    assert!(!is_under_dir("/a/App.vue", "/a/app"));
}

#[test]
fn is_under_dir_canonicalizes_both_sides() {
    assert!(is_under_dir("C:\\proj\\src\\a.ts", "c:/proj"));
}

// ── root-prefix boundary: a root that itself ends in `/` (the canonical roots
//    `/` and `x:/`) — the byte after the prefix is a real path char, not `/`. ──

#[test]
fn is_under_dir_filesystem_root_contains_everything() {
    // FIX 1 regression: "/" is the root; "/foo" IS under it.
    assert!(is_under_dir("/foo", "/"));
    assert!(is_under_dir("/foo/bar", "/"));
}

#[test]
fn is_under_dir_drive_root_contains_drive_paths() {
    // FIX 1 regression: "c:/" is the drive-root; "c:/foo" IS under it.
    assert!(is_under_dir("c:/foo", "c:/"));
    assert!(is_under_dir("c:/foo/bar", "c:/"));
}

#[test]
fn is_under_dir_rejects_non_slash_boundary_sibling() {
    // Boundary must still reject a shared-prefix sibling that is not at a
    // directory boundary.
    assert!(!is_under_dir("/a/project-extra", "/a/project"));
    assert!(!is_under_dir("/foobar", "/foo"));
    assert!(!is_under_dir("c:/project-extra", "c:/project"));
}

#[test]
fn starts_with_dir_filesystem_root_contains_everything() {
    let root = CanonicalPath::new("/");
    assert!(CanonicalPath::new("/foo").starts_with_dir(&root));
    assert!(CanonicalPath::new("/foo/bar").starts_with_dir(&root));
}

#[test]
fn starts_with_dir_drive_root_contains_drive_paths() {
    let root = CanonicalPath::new("c:/");
    assert!(CanonicalPath::new("c:/foo").starts_with_dir(&root));
    assert!(CanonicalPath::new("c:/foo/bar").starts_with_dir(&root));
    // negative: sibling-prefix at drive root is still rejected.
    let project = CanonicalPath::new("c:/project");
    assert!(!CanonicalPath::new("c:/project-extra").starts_with_dir(&project));
}

// ── CanonicalPath::starts_with_dir ──

#[test]
fn canonical_path_starts_with_dir_boundary() {
    let path = CanonicalPath::new("d:/project/src/foo.vue");
    let prefix = CanonicalPath::new("d:/project");
    assert!(path.starts_with_dir(&prefix));

    let sibling = CanonicalPath::new("d:/project-extra/foo.vue");
    assert!(!sibling.starts_with_dir(&prefix));
}

// ── longest_project_root: sort-independent longest, fallback ──

#[test]
fn longest_project_root_picks_longest_regardless_of_order() {
    let forward = vec!["/a".to_string(), "/a/b".to_string()];
    let reversed = vec!["/a/b".to_string(), "/a".to_string()];
    let ws = "/ws";
    assert_eq!(
        longest_project_root("/a/b/c.ts", &forward, ws).as_ref(),
        "/a/b"
    );
    assert_eq!(
        longest_project_root("/a/b/c.ts", &reversed, ws).as_ref(),
        "/a/b"
    );
}

#[test]
fn longest_project_root_falls_back_to_workspace_root() {
    let roots = vec!["/a".to_string(), "/a/b".to_string()];
    assert_eq!(longest_project_root("/x/y", &roots, "/ws").as_ref(), "/ws");
}

#[test]
fn longest_project_root_picks_longest_canonical_not_raw_length() {
    // FIX 2 regression: a raw `//?/` extended root has an inflated raw length
    // that can beat a genuinely deeper canonical root. The contract is longest
    // CANONICAL root. `//?/C:/r` canonicalizes to `c:/r` (len 4) and `c:/r/p`
    // (len 6) is the deeper match — but raw `//?/C:/r` (len 8) is longer.
    let roots = vec!["//?/C:/r".to_string(), "c:/r/p".to_string()];
    assert_eq!(
        longest_project_root("c:/r/p/a.ts", &roots, "ws").as_ref(),
        "c:/r/p"
    );
    // order-independence: reversed roots pick the same canonical-deeper root.
    let reversed = vec!["c:/r/p".to_string(), "//?/C:/r".to_string()];
    assert_eq!(
        longest_project_root("c:/r/p/a.ts", &reversed, "ws").as_ref(),
        "c:/r/p"
    );
}

#[test]
fn longest_project_root_returns_winning_root_in_canonical_form() {
    // The winning root is returned CANONICAL, never the raw stored form — a
    // caller that retained a raw extended-prefix root must not get it leaked
    // back (e.g. as a `projectRootPath`). `//?/C:/repo/pkg` is the sole match
    // for `c:/repo/pkg/a.ts`; the helper returns canonical `c:/repo/pkg`.
    let roots = vec!["//?/C:/repo/pkg".to_string()];
    let got = longest_project_root("c:/repo/pkg/a.ts", &roots, "ws");
    assert_eq!(got.as_ref(), "c:/repo/pkg");
    assert_ne!(got.as_ref(), "//?/C:/repo/pkg");
    // The fallback is likewise canonicalized.
    let none: Vec<String> = vec![];
    assert_eq!(
        longest_project_root("c:/x", &none, "//?/C:/ws").as_ref(),
        "c:/ws"
    );
}

#[test]
fn longest_project_root_rejects_sibling_prefix_match() {
    // sibling /a/bc/d.ts must not falsely match root /a/b → falls back.
    let roots = vec!["/a/b".to_string()];
    assert_eq!(
        longest_project_root("/a/bc/d.ts", &roots, "/ws").as_ref(),
        "/ws"
    );
}

// ── moved CanonicalPath unit coverage (adjusted to c:/ semantics) ──

#[test]
fn backslash_to_forward_slash() {
    let p = CanonicalPath::new("d:\\project\\src\\foo.vue");
    assert_eq!(p.as_str(), "d:/project/src/foo.vue");
    assert!(!p.as_str().contains('\\'));
}

#[test]
fn lowercase_drive_letter_for_windows_style_paths() {
    let p = CanonicalPath::new("D:/Project/src/foo.vue");
    assert_eq!(p.as_str(), "d:/Project/src/foo.vue");
    assert!(p.as_str().contains("Project"), "components NOT lowercased");
}

#[cfg(not(windows))]
#[test]
fn no_case_transform_on_linux() {
    let p = CanonicalPath::new("/home/User/Project/foo.vue");
    assert_eq!(p.as_str(), "/home/User/Project/foo.vue");
}

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
    assert!(set.contains(&CanonicalPath::new("d:/project/foo.vue")));
}

#[test]
fn display_and_conversions() {
    let p = CanonicalPath::new("d:/project/foo.vue");
    assert_eq!(format!("{}", p), p.as_str());
    let from_str: CanonicalPath = "d:\\project\\foo.vue".into();
    assert_eq!(from_str, p);
    let from_string: CanonicalPath = String::from("d:\\project\\foo.vue").into();
    assert_eq!(from_string, p);
    assert!(p.into_string().contains("project/foo.vue"));
}

#[test]
fn empty_string_produces_empty() {
    assert_eq!(CanonicalPath::new("").as_str(), "");
}

#[test]
fn trailing_slash_on_prefix_stripped_then_matches() {
    let path = CanonicalPath::new("d:/project/src/foo.vue");
    let prefix = CanonicalPath::new("d:/project/");
    assert!(path.starts_with_dir(&prefix));
}
