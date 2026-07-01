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

// ── filesystem case-identity policy (the single FS-case authority) ──

/// The unified policy is case-INSENSITIVE on Windows AND default macOS (APFS),
/// case-SENSITIVE on Linux. This must agree with the carrier-store folding policy
/// (`fs_is_case_insensitive() == cfg!(any(windows, macos))`); the pre-fix tsgo
/// `path_eq` diverged by folding on Windows ONLY.
#[test]
fn fs_is_case_insensitive_folds_on_windows_and_default_macos_only() {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
    assert!(
        fs_is_case_insensitive(),
        "Windows + default macOS filesystems fold case"
    );
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    assert!(
        !fs_is_case_insensitive(),
        "Linux filesystems are case-sensitive"
    );
}

/// The pure FS-identity comparison, parameterized by the case-sensitivity bit so it
/// runs and discriminates on EVERY host. Folding happens iff `case_insensitive`.
#[test]
fn fs_paths_equal_under_folds_iff_case_insensitive() {
    // Case-insensitive FS (Windows / default macOS): a case variant is the SAME file.
    assert!(fs_paths_equal_under("/ws/src/A.ts", "/ws/src/a.ts", true));
    // Case-sensitive FS (Linux): a case variant is DISTINCT.
    assert!(!fs_paths_equal_under("/ws/src/A.ts", "/ws/src/a.ts", false));
    // Slash normalization is unconditional (backslash vs forward-slash, any host).
    assert!(fs_paths_equal_under(r"C:\ws\A.ts", "c:/ws/A.ts", true));
    assert!(fs_paths_equal_under(r"C:\ws\A.ts", "C:/ws/A.ts", false));
    // Distinct basenames never match, regardless of the case policy.
    assert!(!fs_paths_equal_under("/ws/src/A.ts", "/ws/src/B.ts", true));
}

/// Discriminating macOS-membership witness, runnable on EVERY host: the bug was the
/// pre-fix tsgo `path_eq` folding case on Windows ONLY (`cfg!(target_os = "windows")`).
/// On macOS (default case-insensitive APFS) that predicate evaluated FALSE, so a
/// carrier whose engine-reported path differed only by case from the configured
/// `root_files` entry compared UNEQUAL and was silently dropped from project
/// membership (empty diagnostics). Modeled here with explicit policy values so the
/// divergence is provable without a macOS host: the OLD macOS policy value (`false`)
/// MISSES, the unified macOS policy value (`true`) HOLDS.
#[test]
fn macos_case_variant_membership_holds_under_unified_policy_not_old_windows_only() {
    let configured = "/ws/src/A.ts";
    let engine_reported = "/ws/src/a.ts"; // the SAME file on case-insensitive APFS

    // OLD predicate on a macOS host: `cfg!(target_os = "windows") == false` ⇒
    // case-sensitive ⇒ the carrier MISSES membership (the regression).
    assert!(
        !fs_paths_equal_under(configured, engine_reported, false),
        "regression witness: the old Windows-only predicate drops the macOS case variant"
    );
    // UNIFIED policy on a macOS host: `fs_is_case_insensitive() == true` ⇒ folds ⇒
    // the carrier stays a configured-project member.
    assert!(
        fs_paths_equal_under(configured, engine_reported, true),
        "the unified policy folds case on macOS so the carrier stays a member"
    );
}

// ── InjectedPathKey filesystem-identity key ──

/// The pure key core, parameterized by the case bit so it discriminates on EVERY
/// host. Exact, slash-divergent, and drive-case-divergent forms of one injected
/// companion ALL fold to the SAME key (regardless of the case policy — the drive
/// fold and slash normalization are unconditional in `canonicalize_path`). A
/// NON-drive-case divergence folds to the same key ONLY on a case-insensitive FS
/// (matching `fs_paths_equal`), and a genuinely-different basename never collides.
#[test]
fn injected_key_under_folds_divergent_forms_to_same_key() {
    // Exact match — trivially the same key under either policy.
    assert_eq!(
        injected_key_under("c:/a/Foo.vue.tsx", true),
        injected_key_under("c:/a/Foo.vue.tsx", true)
    );
    // Slash-divergent (`/` vs `\`) — same key under either policy (unconditional).
    assert_eq!(
        injected_key_under("c:/a/Foo.vue.tsx", true),
        injected_key_under(r"c:\a\Foo.vue.tsx", true),
    );
    assert_eq!(
        injected_key_under("c:/a/Foo.vue.tsx", false),
        injected_key_under(r"c:\a\Foo.vue.tsx", false),
    );
    // Drive-case-divergent (`c:` vs `C:`) — same key under either policy
    // (`canonicalize_path` lowercases the drive letter unconditionally). THIS is
    // the exact reopen case.
    assert_eq!(
        injected_key_under("c:/a/Foo.vue.tsx", true),
        injected_key_under("C:/a/Foo.vue.tsx", true),
    );
    assert_eq!(
        injected_key_under("c:/a/Foo.vue.tsx", false),
        injected_key_under("C:/a/Foo.vue.tsx", false),
    );
    // NON-drive-case divergence (`Foo` vs `foo`): SAME key iff case-insensitive,
    // DISTINCT keys when case-sensitive — exactly the `fs_paths_equal` policy.
    assert_eq!(
        injected_key_under("c:/a/Foo.vue.tsx", true),
        injected_key_under("c:/a/foo.vue.tsx", true),
        "case-insensitive FS folds non-drive case (the same file)"
    );
    assert_ne!(
        injected_key_under("c:/a/Foo.vue.tsx", false),
        injected_key_under("c:/a/foo.vue.tsx", false),
        "case-sensitive FS keeps non-drive-case variants distinct"
    );
    // A genuinely different basename never collides under either policy.
    assert_ne!(
        injected_key_under("c:/a/Foo.vue.tsx", true),
        injected_key_under("c:/a/Bar.vue.tsx", true),
    );
    assert_ne!(
        injected_key_under("c:/a/Foo.vue.tsx", false),
        injected_key_under("c:/a/Bar.vue.tsx", false),
    );
}

/// The public `InjectedPathKey::new` agrees with the host's case policy and with
/// `fs_paths_equal`: two forms `fs_paths_equal` calls the same file produce the
/// same key. Usable in a `HashSet`.
#[test]
fn injected_path_key_matches_fs_policy_and_is_hashable() {
    use std::collections::HashSet;

    // Drive-case + slash divergence: the same file on EVERY host (drive fold +
    // slash normalization are unconditional), so the keys are equal everywhere.
    let a = InjectedPathKey::new("C:\\proj\\Foo.vue.tsx");
    let b = InjectedPathKey::new("c:/proj/Foo.vue.tsx");
    assert_eq!(a, b);
    assert!(fs_paths_equal(
        "C:\\proj\\Foo.vue.tsx",
        "c:/proj/Foo.vue.tsx"
    ));

    let mut set = HashSet::new();
    set.insert(a);
    assert!(
        set.contains(&InjectedPathKey::new("c:/proj/Foo.vue.tsx")),
        "a drive/slash-divergent form of an injected companion is a set member"
    );

    // A NON-drive-case variant tracks the host policy exactly, so the key equality
    // and `fs_paths_equal` never disagree on THIS host.
    let cased_same_file = InjectedPathKey::new("c:/proj/FOO.vue.tsx");
    let base = InjectedPathKey::new("c:/proj/foo.vue.tsx");
    assert_eq!(
        cased_same_file == base,
        fs_paths_equal("c:/proj/FOO.vue.tsx", "c:/proj/foo.vue.tsx"),
        "InjectedPathKey equality must agree with fs_paths_equal on this host"
    );
}
