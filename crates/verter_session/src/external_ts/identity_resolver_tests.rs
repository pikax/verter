//! URI-identity discrimination tests: variant reference forms of ONE tsconfig
//! must resolve to ONE `ProjectIdentity`; genuinely different tsconfigs to
//! different identities; an unsupported/malformed reference to `Unresolved` (the
//! poison path), never a fabricated identity.

use std::sync::Arc;

use super::*;
use crate::external_ts::mode::{
    select_component_mode, EngineSessionCandidates, EngineSessionFacts, OwnedReason,
    OwnedSessionFacts, ProjectEligibility, RedirectRef, ServeMode, SharedSessionFacts,
};
use crate::file_artifact_store::ProjectIdentity;
use verter_span::path::{fs_is_case_insensitive, InjectedPathKey};

/// A FOLDED canonical-lookup identity source: the identity is a deterministic
/// hash of the FS-policy fold of the canonical config path. Mirrors the
/// authoritative source (`host_view_project_identity_for` + the workspace's
/// case-folding owner resolution): two canonical paths that denote the same file
/// under the host case policy yield the SAME identity; distinct files yield
/// distinct identities.
fn folded_identity(canonical: &str) -> ProjectIdentity {
    let key = InjectedPathKey::new(canonical);
    ProjectIdentity(xxhash_rust::xxh3::xxh3_128(key.as_str().as_bytes()).to_le_bytes())
}

/// A probe with no symlinks: every config path is used as-is (realpath miss).
fn no_realpath(_canonical: &str) -> Option<String> {
    None
}

/// Resolve a reference (declared from `dir`) with no symlink probe, through the
/// folded identity source.
fn resolve(reference: &str, dir: &str) -> RedirectRef {
    resolve_reference_identity(reference, dir, &no_realpath, &folded_identity)
}

fn candidates() -> EngineSessionCandidates {
    let facts = |version: &str, pin: u64, generation: u64| EngineSessionFacts {
        observed_version: Arc::<str>::from(version),
        wire_pin: pin,
        editor_session_generation: generation,
    };
    EngineSessionCandidates {
        owned: OwnedSessionFacts::new(facts("7.0.1", 1, 0)),
        shared: Some(SharedSessionFacts::new(facts("7.0.1", 7, 3))),
    }
}

// ── One tsconfig, many reference forms → one identity ──

/// Relative, explicit-`.json`, `.`/`..`-laden, backslash, and `file://` variants
/// of the SAME `c:/repo/lib/tsconfig.json` all resolve to ONE identity.
#[test]
fn variant_reference_forms_of_one_tsconfig_resolve_to_a_single_identity() {
    let dir = "c:/repo/app";
    let want = RedirectRef::Resolved(folded_identity("c:/repo/lib/tsconfig.json"));

    // Directory reference (append tsconfig.json).
    assert_eq!(resolve("../lib", dir), want, "bare directory reference");
    // Explicit tsconfig file.
    assert_eq!(
        resolve("../lib/tsconfig.json", dir),
        want,
        "explicit tsconfig.json"
    );
    // Leading `./` and interior `.` segments.
    assert_eq!(resolve("./../lib", dir), want, "leading ./");
    assert_eq!(resolve("../lib/./tsconfig.json", dir), want, "interior .");
    // Interior `..` that cancels out.
    assert_eq!(
        resolve("../lib/../lib/tsconfig.json", dir),
        want,
        "interior .. that cancels"
    );
    assert_eq!(resolve("../foo/../lib", dir), want, "sibling .. detour");
    // Backslash separators (Windows path form).
    assert_eq!(resolve(r"..\lib\tsconfig.json", dir), want, "backslashes");
    // Absolute file:// URI (the referencing dir is irrelevant for an absolute ref).
    assert_eq!(
        resolve("file:///c:/repo/lib/tsconfig.json", dir),
        want,
        "file:// URI"
    );
    // An UPPERCASE drive in the file:// URI still lowercases the drive letter.
    assert_eq!(
        resolve("file:///C:/repo/lib/tsconfig.json", dir),
        want,
        "file:// URI with uppercase drive"
    );
}

/// Mixed NON-drive case (`LIB` vs `lib`) resolves to ONE identity on a
/// case-insensitive volume (Windows / default macOS) and to TWO on a
/// case-sensitive one (Linux) — the folded-canonical-lookup honoring the host
/// case policy, never a blanket lowercasing that would conflate distinct
/// case-sensitive files.
#[test]
fn mixed_case_reference_follows_the_host_case_policy() {
    let dir = "c:/repo/app";
    let lower = resolve("../lib/tsconfig.json", dir);
    let upper = resolve("../LIB/tsconfig.json", dir);
    if fs_is_case_insensitive() {
        assert_eq!(
            lower, upper,
            "a case-insensitive volume folds LIB and lib to one identity"
        );
    } else {
        assert_ne!(
            lower, upper,
            "a case-sensitive volume keeps LIB and lib as distinct files"
        );
    }
}

/// Two genuinely different tsconfigs resolve to DIFFERENT identities (the
/// negative: the resolver is not collapsing everything to one bucket).
#[test]
fn different_tsconfigs_resolve_to_different_identities() {
    let dir = "c:/repo/app";
    let lib = resolve("../lib", dir);
    let other = resolve("../other", dir);
    assert_ne!(lib, other);
    assert_eq!(
        lib,
        RedirectRef::Resolved(folded_identity("c:/repo/lib/tsconfig.json"))
    );
    assert_eq!(
        other,
        RedirectRef::Resolved(folded_identity("c:/repo/other/tsconfig.json"))
    );
}

/// A symlinked config directory resolves to the REALPATH target's identity, so a
/// reference through the symlink and a direct reference to the target share ONE
/// identity. Uses the injected `ConfigPathProbe` (headless, always runs); the
/// real-FS symlink primitive is covered by `verter_workspace`'s realpath tests.
#[test]
fn symlinked_config_resolves_to_the_realpath_target_identity() {
    let dir = "c:/repo/app";
    // `c:/repo/link` is a symlink to `c:/repo/lib`; realpath rewrites the config
    // path the resolver produced for the symlinked directory reference.
    let probe = |canonical: &str| -> Option<String> {
        if canonical == "c:/repo/link/tsconfig.json" {
            Some("c:/repo/lib/tsconfig.json".to_string())
        } else {
            None
        }
    };
    let through_link = resolve_reference_identity("../link", dir, &probe, &folded_identity);
    let direct = resolve("../lib", dir);
    assert_eq!(
        through_link, direct,
        "a symlinked config and its target share one identity"
    );
    assert_eq!(
        through_link,
        RedirectRef::Resolved(folded_identity("c:/repo/lib/tsconfig.json"))
    );
}

/// A REAL on-disk symlink (created in a tempdir) and its target resolve to ONE
/// identity through a realpath-backed probe. Skipped ONLY when the OS/permission
/// model genuinely cannot create a symlink (e.g. Windows without developer mode);
/// the headless legs above always run regardless.
#[test]
fn real_filesystem_symlink_resolves_to_one_identity() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let base = tmp.path();
    let target_dir = base.join("lib");
    std::fs::create_dir_all(&target_dir).expect("create target dir");
    std::fs::write(target_dir.join("tsconfig.json"), "{}").expect("write tsconfig");

    let link_dir = base.join("link");
    if !create_dir_symlink(&target_dir, &link_dir) {
        eprintln!("skipping real-symlink leg: OS/permissions cannot create a directory symlink");
        return;
    }

    let probe = |canonical: &str| -> Option<String> {
        std::fs::canonicalize(canonical)
            .ok()
            .map(|p| verter_span::path::canonicalize_path(&p.to_string_lossy()))
    };

    let base_dir = verter_span::path::canonicalize_path(&base.to_string_lossy());
    let through_link = resolve_reference_identity("link", &base_dir, &probe, &folded_identity);
    let direct = resolve_reference_identity("lib", &base_dir, &probe, &folded_identity);
    assert_eq!(
        through_link, direct,
        "a real on-disk symlink and its target resolve to one identity"
    );
    assert!(matches!(through_link, RedirectRef::Resolved(_)));
}

#[cfg(windows)]
fn create_dir_symlink(target: &std::path::Path, link: &std::path::Path) -> bool {
    std::os::windows::fs::symlink_dir(target, link).is_ok()
}

#[cfg(unix)]
fn create_dir_symlink(target: &std::path::Path, link: &std::path::Path) -> bool {
    std::os::unix::fs::symlink(target, link).is_ok()
}

#[cfg(not(any(windows, unix)))]
fn create_dir_symlink(_target: &std::path::Path, _link: &std::path::Path) -> bool {
    false
}

// ── Unsupported / malformed references are Unresolved (poison), not fabricated ──

/// An unsupported scheme, or a malformed `file:` without `//`, resolves to
/// `Unresolved` — never a fabricated identity. A bare drive path (`c:/…`) is NOT
/// a scheme and still resolves.
#[test]
fn unsupported_or_malformed_scheme_references_are_unresolved() {
    let dir = "c:/repo/app";
    for reference in [
        "untitled:Untitled-1",
        "http://example.com/tsconfig.json",
        "vscode-vfs://host/repo/tsconfig.json",
        "file:/c:/repo/lib/tsconfig.json", // malformed: no `//`
    ] {
        assert_eq!(
            resolve(reference, dir),
            RedirectRef::Unresolved,
            "reference {reference:?} must fail closed to Unresolved"
        );
    }

    // A bare drive path is a path, not a scheme — it resolves normally.
    assert_eq!(
        resolve("c:/repo/lib/tsconfig.json", dir),
        RedirectRef::Resolved(folded_identity("c:/repo/lib/tsconfig.json")),
        "a bare drive path is not an unsupported scheme"
    );
}

/// A reference the resolver could not canonicalize (unsupported scheme) becomes
/// an `Unresolved` graph entry that poisons SHARED per `mode`: the declaring
/// project's whole component fails closed to `IncompleteComponent`, and a
/// SEPARATE eligible component is poisoned snapshot-wide with
/// `UnresolvedRedirectInSnapshot`. This ties the resolver's poison output to the
/// mode layer's fail-closed contract.
#[test]
fn unresolved_reference_poisons_shared_through_the_mode_layer() {
    let a = folded_identity("c:/repo/app/tsconfig.json");
    let b = folded_identity("c:/repo/sep/tsconfig.json");

    // `a` declares one unsupported-scheme reference (→ Unresolved) plus is itself
    // eligible; `b` is a separate, independently-eligible project with no edge.
    let a_refs = [ReferenceInput::redirect_on("untitled:broken")];
    let b_refs: [ReferenceInput; 0] = [];
    let projects = [
        ProjectGraphInput {
            identity: a,
            eligibility: ProjectEligibility::Eligible,
            tsconfig_dir: "c:/repo/app",
            references: &a_refs,
        },
        ProjectGraphInput {
            identity: b,
            eligibility: ProjectEligibility::Eligible,
            tsconfig_dir: "c:/repo/sep",
            references: &b_refs,
        },
    ];
    let graph = build_redirect_reference_graph(&projects, &no_realpath, &folded_identity);

    // Declaring side: `a`'s own component fails closed member-local.
    let from_a = select_component_mode(&graph, &a, &candidates());
    assert_eq!(from_a.mode(), ServeMode::Owned);
    assert_eq!(
        from_a.owned_reason(),
        Some(OwnedReason::IncompleteComponent)
    );

    // Target side: the separate eligible `b` is poisoned snapshot-wide.
    let from_b = select_component_mode(&graph, &b, &candidates());
    assert_eq!(from_b.mode(), ServeMode::Owned);
    assert_eq!(
        from_b.owned_reason(),
        Some(OwnedReason::UnresolvedRedirectInSnapshot)
    );
}

// ── Non-ASCII reference paths never byte-slice-panic (char-boundary safety) ──

/// A reference whose collapsed path ends INSIDE a multibyte UTF-8 sequence at the
/// `.json`-suffix probe offset must NOT panic on the live-decision
/// canonicalization path. The config-vs-directory classifier inspects the
/// trailing bytes for a `.json` extension; a byte-offset `&str` slice there
/// unwinds when the offset is not a char boundary. The classifier must be
/// char-boundary safe and fail closed: a non-`.json` (directory) reference with a
/// multibyte tail resolves to that directory's `tsconfig.json` identity, never a
/// panic.
///
/// Discriminating: pre-fix the `path[path.len() - 5..]` slice panics ("byte index
/// N is not a char boundary") for both tails below; post-fix both resolve.
#[test]
fn multibyte_tail_reference_never_byte_slice_panics_and_resolves_directory() {
    let dir = "c:/repo/app";

    // Tail "é😀" = bytes [C3 A9 F0 9F 98 80]; the 5-bytes-from-end `.json` probe
    // offset lands on the 0xA9 UTF-8 continuation byte → a byte-offset `&str`
    // slice there panics. Not a `.json` path → directory reference → tsconfig.json.
    let resolved = resolve("../lib-\u{00E9}\u{1F600}", dir);
    assert_eq!(
        resolved,
        RedirectRef::Resolved(folded_identity(
            "c:/repo/lib-\u{00E9}\u{1F600}/tsconfig.json"
        )),
        "a multibyte-tail directory reference resolves fail-closed to its \
         tsconfig.json identity without unwinding"
    );

    // A second straddling tail "€€" = bytes [E2 82 AC E2 82 AC]; the probe offset
    // lands on a different (0x82) continuation byte and must likewise never panic.
    let euro = resolve("../pkg-\u{20AC}\u{20AC}", dir);
    assert_eq!(
        euro,
        RedirectRef::Resolved(folded_identity(
            "c:/repo/pkg-\u{20AC}\u{20AC}/tsconfig.json"
        )),
        "a second multibyte-tail directory reference also resolves without panic"
    );

    // Regression: a NORMAL ASCII `.json` reference is still classified as the
    // config FILE itself (not re-suffixed with /tsconfig.json), so the explicit
    // and directory forms share one identity.
    let explicit = resolve("../lib/tsconfig.json", dir);
    let directory = resolve("../lib", dir);
    assert_eq!(
        explicit, directory,
        "an ASCII `.json` reference still resolves as the config file (no regression)"
    );
    assert_eq!(
        explicit,
        RedirectRef::Resolved(folded_identity("c:/repo/lib/tsconfig.json"))
    );

    // Regression: a mixed-case `.JSON` extension is still classified as a config
    // file (case-insensitive on the extension) — the char-safe check must keep the
    // original case-insensitivity, not silently narrow to case-sensitive.
    let upper_ext = resolve("../lib/tsconfig.JSON", dir);
    assert!(
        matches!(upper_ext, RedirectRef::Resolved(_)),
        "a mixed-case .JSON extension still classifies as a config file"
    );
    assert_ne!(
        upper_ext,
        RedirectRef::Resolved(folded_identity("c:/repo/lib/tsconfig.json/tsconfig.json")),
        "a .JSON reference is NOT treated as a directory (no /tsconfig.json append)"
    );
}

// ── Graph construction: canonical edges, redirect-disabled exclusion ──

/// The graph builder resolves references to canonical-identity edges BEFORE
/// insertion, so two eligible projects joined by a resolved reference form ONE
/// SHARED component.
#[test]
fn graph_builder_resolves_references_to_canonical_edges() {
    let app = folded_identity("c:/repo/app/tsconfig.json");
    let lib = folded_identity("c:/repo/lib/tsconfig.json");
    let app_refs = [ReferenceInput::redirect_on("../lib")];
    let lib_refs: [ReferenceInput; 0] = [];
    let projects = [
        ProjectGraphInput {
            identity: app,
            eligibility: ProjectEligibility::Eligible,
            tsconfig_dir: "c:/repo/app",
            references: &app_refs,
        },
        ProjectGraphInput {
            identity: lib,
            eligibility: ProjectEligibility::Eligible,
            tsconfig_dir: "c:/repo/lib",
            references: &lib_refs,
        },
    ];
    let graph = build_redirect_reference_graph(&projects, &no_realpath, &folded_identity);

    let component: Vec<_> = graph.connected_component(&app).members().collect();
    assert!(component.contains(&app) && component.contains(&lib));
    assert_eq!(component.len(), 2, "the resolved reference joined the two");

    let decision = select_component_mode(&graph, &app, &candidates());
    assert_eq!(decision.mode(), ServeMode::Shared);
}

/// A reference under `disableSourceOfProjectReferenceRedirect: true` is EXCLUDED
/// by the builder — it forms no edge, so the referenced project is not a
/// component member and the two decide independently.
#[test]
fn redirect_disabled_reference_is_excluded_from_the_graph() {
    let app = folded_identity("c:/repo/app/tsconfig.json");
    let lib = folded_identity("c:/repo/lib/tsconfig.json");
    // A single redirect-DISABLED reference to lib.
    let app_refs = [ReferenceInput::redirect_disabled("../lib")];
    let projects = [ProjectGraphInput {
        identity: app,
        eligibility: ProjectEligibility::Eligible,
        tsconfig_dir: "c:/repo/app",
        references: &app_refs,
    }];
    let graph = build_redirect_reference_graph(&projects, &no_realpath, &folded_identity);

    let component: Vec<_> = graph.connected_component(&app).members().collect();
    assert_eq!(
        component,
        vec![app],
        "a redirect-disabled reference is not a graph edge"
    );
    assert!(!component.contains(&lib));
}
