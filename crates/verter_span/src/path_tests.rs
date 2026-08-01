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
    // The DRIVE fold is unconditional too — `canonicalize_path` lowercases the drive
    // letter on every host, so a drive-case divergence is the same file even under
    // the case-SENSITIVE policy. Slash-only normalization made this pair compare
    // unequal on Linux while `InjectedPathKey` keyed it equal.
    assert!(fs_paths_equal_under(r"C:\ws\A.ts", "c:/ws/A.ts", false));
    // Extended-length prefix and trailing slash are likewise folded on every host.
    assert!(fs_paths_equal_under(r"\\?\C:\ws\A.ts", "c:/ws/A.ts", false));
    assert!(fs_paths_equal_under("/ws/src/", "/ws/src", false));
    // Distinct basenames never match, regardless of the case policy.
    assert!(!fs_paths_equal_under("/ws/src/A.ts", "/ws/src/B.ts", true));
    // NEGATIVE: canonicalizing must not make the case-sensitive branch permissive —
    // a non-drive case variant stays DISTINCT.
    assert!(!fs_paths_equal_under("/ws/src/A.ts", "/ws/src/a.ts", false));
    assert!(!fs_paths_equal_under(r"C:\ws\A.ts", "c:/ws/a.ts", false));
}

/// The agreement invariant, proven on EVERY host under BOTH policy values:
/// `InjectedPathKey` equality and `fs_paths_equal` are the same relation, because
/// both derive from the one shared `canonicalize_path` normalization.
///
/// Discriminating: with the pre-fix slash-only comparison core, every drive-case /
/// extended-prefix / trailing-slash row below disagrees under `case_insensitive =
/// false` (the key folds, the predicate does not). The case-insensitive branch
/// agreed by accident — `eq_ignore_ascii_case` folds the drive letter too — which is
/// exactly why this class of bug reached CI as a Linux-only failure.
#[test]
fn key_equality_and_fs_paths_equal_are_the_same_relation_under_both_policies() {
    // (left, right) pairs spanning every normalization axis plus genuine distinctness.
    let pairs = [
        (r"C:\proj\Foo.vue.tsx", "c:/proj/Foo.vue.tsx"),
        (r"C:\ws\A.ts", "c:/ws/A.ts"),
        (r"\\?\C:\ws\A.ts", "c:/ws/A.ts"),
        ("/ws/src/", "/ws/src"),
        ("/ws/src/A.ts", "/ws/src/a.ts"),
        ("c:/proj/FOO.vue.tsx", "c:/proj/foo.vue.tsx"),
        ("/ws/src/A.ts", "/ws/src/B.ts"),
        ("c:/a/Foo.vue.tsx", "d:/a/Foo.vue.tsx"),
    ];
    for case_insensitive in [true, false] {
        for (left, right) in pairs {
            let keys_equal = injected_key_under(left, case_insensitive)
                == injected_key_under(right, case_insensitive);
            let paths_equal = fs_paths_equal_under(left, right, case_insensitive);
            assert_eq!(
                keys_equal, paths_equal,
                "InjectedPathKey equality must equal fs_paths_equal for \
                 ({left:?}, {right:?}) under case_insensitive={case_insensitive}"
            );
        }
    }
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

// ── simplify_verbatim_path: the EXEC-boundary transform ─────────────────────
//
// Every case below constructs the `\\?\` input EXPLICITLY as a string rather
// than calling `canonicalize()`, so the rule is exercised identically on macOS,
// Linux, and Windows. `canonicalize()` only *produces* this shape on Windows;
// what the shape must *become* is a host-independent fact, and that is what the
// production exec boundary depends on.

#[test]
fn verbatim_disk_prefix_is_stripped() {
    assert_eq!(
        simplify_verbatim_path_str(r"\\?\D:\dev\app\node_modules\typescript\lib\tsserver.js"),
        r"D:\dev\app\node_modules\typescript\lib\tsserver.js"
    );
    // NEGATIVE: the prefix that kills node's `resolveMainPath` is gone, and the
    // drive was not eaten along with it (the `EISDIR: lstat 'D:'` shape).
    let out = simplify_verbatim_path_str(r"\\?\D:\dev\app\tsserver.js");
    assert!(
        !out.starts_with(r"\\?"),
        "no verbatim prefix survives: {out}"
    );
    assert!(out.starts_with(r"D:\"), "the drive root survives: {out}");
}

#[test]
fn verbatim_unc_prefix_becomes_a_plain_unc_path() {
    assert_eq!(
        simplify_verbatim_path_str(r"\\?\UNC\build01\share\ws\tsserver.js"),
        r"\\build01\share\ws\tsserver.js"
    );
    // NEGATIVE: the naive `strip_prefix(r"\\?\")` answer is a RELATIVE path
    // beginning with a literal `UNC` directory — a silent corruption.
    assert_ne!(
        simplify_verbatim_path_str(r"\\?\UNC\build01\share\ws\tsserver.js"),
        r"UNC\build01\share\ws\tsserver.js"
    );
}

#[test]
fn already_simple_and_posix_paths_are_returned_untouched_and_borrowed() {
    for path in [
        r"D:\dev\app\tsserver.js",
        r"\\build01\share\ws\tsserver.js",
        "/usr/local/lib/node_modules/typescript/lib/tsserver.js",
        "relative/tsserver.js",
        "",
    ] {
        let out = simplify_verbatim_path_str(path);
        assert_eq!(out, path, "{path} must be unchanged");
        assert!(
            matches!(out, Cow::Borrowed(_)),
            "{path} needs no allocation"
        );
    }
}

#[test]
fn unsimplifiable_verbatim_paths_are_left_verbatim_never_corrupted() {
    // A device-namespace verbatim path has no Win32 spelling at all.
    let volume = r"\\?\Volume{9c8f1e2a-0000-0000-0000-100000000000}\ws\tsserver.js";
    assert_eq!(simplify_verbatim_path_str(volume), volume);

    // Win32 TRIMS a trailing dot/space, so the simplified form would name a
    // different file (or none).
    let trailing_dot = r"\\?\D:\ws\odd.\tsserver.js";
    assert_eq!(simplify_verbatim_path_str(trailing_dot), trailing_dot);
    let trailing_space = r"\\?\D:\ws\odd \tsserver.js";
    assert_eq!(simplify_verbatim_path_str(trailing_space), trailing_space);

    // Win32 RESOLVES `.`/`..`; verbatim keeps them literal.
    let dotdot = r"\\?\D:\ws\..\tsserver.js";
    assert_eq!(simplify_verbatim_path_str(dotdot), dotdot);

    // A reserved DOS device name in any component.
    let device = r"\\?\D:\ws\NUL\tsserver.js";
    assert_eq!(simplify_verbatim_path_str(device), device);
    let device_ext = r"\\?\D:\ws\com1.js";
    assert_eq!(simplify_verbatim_path_str(device_ext), device_ext);

    // `/` is a literal filename character under the verbatim prefix.
    let literal_slash = r"\\?\D:\ws\a/b\tsserver.js";
    assert_eq!(simplify_verbatim_path_str(literal_slash), literal_slash);

    // Longer than MAX_PATH: only the verbatim form can name it.
    let long = format!(r"\\?\D:\{}\tsserver.js", "d".repeat(300));
    assert_eq!(simplify_verbatim_path_str(&long), long);

    // NEGATIVE: none of the refusals silently produced a different path.
    for input in [volume, trailing_dot, dotdot, device, literal_slash] {
        assert!(
            matches!(simplify_verbatim_path_str(input), Cow::Borrowed(_)),
            "{input} must be returned as-is, not rewritten"
        );
    }
}

#[test]
fn simplify_is_idempotent_and_agrees_across_the_str_and_path_apis() {
    for input in [
        r"\\?\D:\ws\tsserver.js",
        r"\\?\UNC\srv\share\tsserver.js",
        r"\\?\Volume{0}\ws\tsserver.js",
        "/usr/lib/tsserver.js",
    ] {
        let once = simplify_verbatim_path_str(input).into_owned();
        assert_eq!(
            simplify_verbatim_path_str(&once),
            once,
            "{input} must be a fixed point after one pass"
        );
        assert_eq!(
            simplify_verbatim_path(std::path::Path::new(input)),
            std::path::Path::new(&once),
            "the Path API must agree with the str API for {input}"
        );
    }
}

#[test]
fn simplify_does_not_canonicalize_case_or_separators() {
    // The exec boundary is NOT the canonical-ID boundary: the drive keeps its
    // case and the separators stay native. (`canonicalize_path` owns that, and
    // conflating the two would change cache-key identity.)
    assert_eq!(
        simplify_verbatim_path_str(r"\\?\D:\Dev\App.vue"),
        r"D:\Dev\App.vue"
    );
    assert_ne!(
        simplify_verbatim_path_str(r"\\?\D:\Dev\App.vue"),
        "d:/Dev/App.vue"
    );
}

// ── MAX_PATH is a CHARACTER limit, not a UTF-8 byte limit ───────────────────

#[test]
fn a_non_ascii_path_under_the_character_limit_still_simplifies() {
    // Win32 measures `MAX_PATH` in UTF-16 code units, not UTF-8 bytes. A path
    // of ~130 accented characters is comfortably legal on Windows while being
    // over 260 UTF-8 bytes. Refusing it would hand node the untouched `\\?\`
    // argument — the very `EISDIR` outage this helper exists to prevent — for
    // every user with a non-ASCII install path.
    let dir = "é".repeat(130);
    let input = format!(r"\\?\D:\{dir}\x.js");
    let expected = format!(r"D:\{dir}\x.js");
    assert!(
        expected.len() > 260,
        "the fixture must exceed the BYTE limit to discriminate ({} bytes)",
        expected.len()
    );
    assert!(
        expected.chars().map(char::len_utf16).sum::<usize>() < 260,
        "the fixture must be under the real CHARACTER limit"
    );
    assert_eq!(simplify_verbatim_path_str(&input), expected);
}

#[test]
fn over_the_character_limit_is_still_refused_and_the_boundary_is_utf16() {
    // Just over: 256 units of directory + `D:\` + `\x` = 261 units.
    let long = format!(r"\\?\D:\{}\x", "é".repeat(256));
    assert_eq!(
        simplify_verbatim_path_str(&long),
        long,
        "261 units must refuse"
    );
    // Just under: 254 units of directory ⇒ 259 units total.
    let ok = format!(r"\\?\D:\{}\x", "é".repeat(254));
    assert_eq!(
        simplify_verbatim_path_str(&ok),
        format!(r"D:\{}\x", "é".repeat(254)),
        "259 units must simplify"
    );
    // A surrogate pair counts as TWO UTF-16 units even though it is one char.
    let astral = format!(r"\\?\D:\{}\x", "𝄞".repeat(128));
    assert_eq!(
        simplify_verbatim_path_str(&astral),
        astral,
        "128 astral chars are 256 UTF-16 units — over the limit with the root"
    );
}

// ── Reserved device names, including the superscript COM/LPT forms ──────────

#[test]
fn superscript_com_and_lpt_device_components_are_refused() {
    // Windows reserves COM¹/COM²/COM³ and LPT¹/LPT²/LPT³ alongside COM1–COM9.
    // Stripping the prefix off a path containing one would rewrite it to a
    // DIFFERENT target (the device), breaking the never-corrupt contract.
    for name in [
        "COM\u{b9}",
        "COM\u{b2}",
        "COM\u{b3}",
        "LPT\u{b9}",
        "LPT\u{b2}",
        "LPT\u{b3}",
    ] {
        let input = format!(r"\\?\D:\ws\{name}\tsserver.js");
        assert_eq!(
            simplify_verbatim_path_str(&input),
            input,
            "{name} is a reserved device component"
        );
        let with_ext = format!(r"\\?\D:\ws\{}.js", name.to_lowercase());
        assert_eq!(
            simplify_verbatim_path_str(&with_ext),
            with_ext,
            "{name}.js resolves to the device too"
        );
    }
    // NEGATIVE: the ASCII-digit neighbours and lookalikes stay ordinary names.
    for ordinary in ["COM0", "COM10", "COMx", "console", "lptop"] {
        let input = format!(r"\\?\D:\ws\{ordinary}\tsserver.js");
        assert_eq!(
            simplify_verbatim_path_str(&input),
            format!(r"D:\ws\{ordinary}\tsserver.js"),
            "{ordinary} is not a device name"
        );
    }
}

#[test]
fn reserved_device_classification_is_shared_and_discriminates() {
    for reserved in [
        &b"NUL"[..],
        b"nul",
        b"nul.txt",
        b"Nul.tar.gz",
        b"COM1",
        b"lpt9.log",
        b"CONIN$",
        b"conin$",
        b"CONOUT$.txt",
        b"CON",
        b"prn",
        b"AUX",
    ] {
        assert!(
            is_reserved_device_name(reserved),
            "{:?}",
            String::from_utf8_lossy(reserved)
        );
    }
    for superscript in ["COM\u{b9}", "com\u{b2}", "LPT\u{b3}", "lpt\u{b9}.log"] {
        assert!(
            is_reserved_device_name(superscript.as_bytes()),
            "{superscript} is reserved"
        );
    }
    for ordinary in [
        &b"conin"[..],
        b"CONOUT",
        b"conout$x",
        b"COM0",
        b"COM10",
        b"console",
        b"nullable.rs",
        b"aux_data",
        b"COM",
        b"LPT",
    ] {
        assert!(
            !is_reserved_device_name(ordinary),
            "{:?}",
            String::from_utf8_lossy(ordinary)
        );
    }
}

// ── The literal grammar must refuse drive-relative and incomplete-UNC bodies ─

#[test]
fn a_bare_drive_verbatim_body_is_refused_because_it_is_drive_relative() {
    // `D:` without a separator is DRIVE-RELATIVE under Win32 — it resolves
    // against drive D's current directory, a different target every time.
    assert_eq!(simplify_verbatim_path_str(r"\\?\D:"), r"\\?\D:");
    // The drive ROOT is fully qualified and must still simplify.
    assert_eq!(simplify_verbatim_path_str(r"\\?\D:\"), r"D:\");
    assert_eq!(simplify_verbatim_path_str(r"\\?\D:\x"), r"D:\x");
}

#[test]
fn an_incomplete_unc_body_is_refused() {
    // `\\server` alone names no share, so it is not a usable Win32 path.
    assert_eq!(simplify_verbatim_path_str(r"\\?\UNC\srv"), r"\\?\UNC\srv");
    assert_eq!(simplify_verbatim_path_str(r"\\?\UNC\srv\"), r"\\?\UNC\srv\");
    assert_eq!(simplify_verbatim_path_str(r"\\?\UNC\"), r"\\?\UNC\");
    // Server + share IS complete.
    assert_eq!(
        simplify_verbatim_path_str(r"\\?\UNC\srv\share"),
        r"\\srv\share"
    );
    assert_eq!(
        simplify_verbatim_path_str(r"\\?\UNC\srv\share\x.js"),
        r"\\srv\share\x.js"
    );
}

// ── The refusal reason is reportable, not silent ────────────────────────────

#[test]
fn verbatim_refusal_names_the_reason_and_is_none_when_simplification_succeeds() {
    use crate::path::VerbatimRefusal;

    assert_eq!(verbatim_refusal(r"\\?\D:\ws\tsserver.js"), None);
    assert_eq!(verbatim_refusal("/usr/lib/tsserver.js"), None);
    assert_eq!(verbatim_refusal(r"D:\ws\tsserver.js"), None);

    assert_eq!(
        verbatim_refusal(r"\\?\Volume{0}\ws\x.js"),
        Some(VerbatimRefusal::DeviceNamespace)
    );
    assert_eq!(
        verbatim_refusal(r"\\?\D:"),
        Some(VerbatimRefusal::DriveRelative)
    );
    assert_eq!(
        verbatim_refusal(r"\\?\UNC\srv"),
        Some(VerbatimRefusal::IncompleteUnc)
    );
    assert!(matches!(
        verbatim_refusal(&format!(r"\\?\D:\{}\x", "d".repeat(300))),
        Some(VerbatimRefusal::TooLong { .. })
    ));
    assert!(matches!(
        verbatim_refusal(r"\\?\D:\ws\NUL\x.js"),
        Some(VerbatimRefusal::Component { .. })
    ));

    // The rendered message names the offending detail — it is user-visible.
    let rendered = verbatim_refusal(r"\\?\D:\ws\NUL\x.js").unwrap().to_string();
    assert!(
        rendered.contains("NUL"),
        "the message must name the component: {rendered}"
    );
    assert!(
        rendered.contains("device"),
        "the message must say WHY: {rendered}"
    );

    // Refusal and simplification are the SAME decision — never disagree.
    for input in [
        r"\\?\D:\ws\x.js",
        r"\\?\D:",
        r"\\?\UNC\srv",
        r"\\?\UNC\srv\share\x",
        r"\\?\Volume{0}\x",
        r"\\?\D:\ws\NUL\x.js",
        r"/usr/lib/x.js",
    ] {
        let changed = matches!(simplify_verbatim_path_str(input), Cow::Owned(_));
        let refused = verbatim_refusal(input).is_some();
        assert!(!(changed && refused), "{input}: simplified AND refused");
        assert_eq!(
            refused,
            input.starts_with(r"\\?\") && !changed,
            "{input}: refusal must be exactly 'verbatim and not simplified'"
        );
    }
}
