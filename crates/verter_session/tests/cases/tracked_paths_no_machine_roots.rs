//! Architecture guard: no git-TRACKED file's content embeds a
//! machine/user/session/orchestration absolute-path marker.
//!
//! Verter is developed, built, and tested on macOS, Windows, AND Linux,
//! by more than one person, across many ephemeral orchestration runs. A
//! tracked file that hardcodes ONE developer's `$HOME`, ONE machine's
//! checkout drive, or ONE orchestration run's scratch dir is a defect:
//! it is meaningless (and often broken) on every other machine and OS.
//! This is the CONTENT-residue half of the Cross-Platform Portability
//! rule; the path-SHAPE half (NTFS-legal components, no case collisions,
//! ≤200-byte paths) lives in the sibling `tracked_paths_are_portable`.
//!
//! SCOPE / CLAIM: this guard does NOT prove the absence of every
//! machine/user/session absolute path. It bans a FIXED set of 64 known
//! leaked-root markers from tracked content — the specific roots that have
//! actually leaked into this repo's tracked files. It is a tombstone for
//! those known roots, not a complete machine-path detector (a complete
//! detector would false-positive the ~70 legitimate cross-platform path
//! fixtures the repo intentionally carries — see DOCUMENTED RESIDUAL).
//!
//! The guard enumerates the tracked tree via `git ls-files -z` (same
//! mechanism as the sibling), reads each file's RAW BYTES, and FAILS if
//! any tracked file's bytes contain any of these 64 fixed-string markers
//! (exact byte-subslice match, no regex, no broadening). The scan is
//! byte-level — NOT lossy-UTF-8 — because every marker is pure ASCII and
//! can therefore appear verbatim inside an otherwise-binary or
//! one-stray-byte-non-UTF-8 tracked blob; gating the scan on `from_utf8`
//! would silently skip exactly those files. The scan is FAIL-CLOSED on
//! BOTH axes: a tracked file that cannot be read is a guard failure (not a
//! skip), AND a tracked path that is not valid UTF-8 is a guard failure
//! (not a silent drop) — matching the sibling's claim. The
//! first 26 markers are:
//!
//!   1.  `/Users/carlosrodrigues`              (this dev's POSIX home)
//!   2.  `/private/tmp/claude`                 (this dev's macOS Claude scratch)
//!   3.  `/tmp/mom`                            (orchestration scratch root)
//!   4.  `/tmp/orch`                           (orchestration scratch root)
//!   5.  `/d/tmp/orch`                         (Git-Bash drive-mapped orch scratch)
//!   6.  `C:/Users/david/.claude`             (another dev's Windows Claude dir, fwd-slash)
//!   7.  `C:\Users\david\.claude`             (same, backslash form — a separate marker)
//!   8.  `/dev/personal/verter`                (this dev's checkout-root tail, any drive)
//!   9.  `D:/dev/personal/verter`             (this dev's Windows checkout root, uppercase)
//!   10. `d:/dev/personal/verter`             (same, lowercase drive)
//!   11. `/mnt/d/dev/personal/verter`         (same, WSL mount)
//!   12. `D:/tmp/`                             (Windows-drive scratch, uppercase)
//!   13. `d:/tmp/`                             (Windows-drive scratch, lowercase)
//!
//! Markers 14-26 close the WINDOWS-BACKSLASH spellings of the SAME two
//! already-classified machine paths (V5 checkout root + V6 drive scratch).
//! Slash direction is only spelling, so each separator/UNC/mixed variant
//! that contains at least one backslash is the same machine-specific path:
//!
//!   14-20.  V5 checkout-root tail, all separator spellings that contain a
//!           backslash — `\dev\personal\verter`, `\dev\personal/verter`,
//!           `\dev/personal\verter`, `\dev/personal/verter`,
//!           `/dev\personal\verter`, `/dev\personal/verter`,
//!           `/dev/personal\verter` (the all-forward `/dev/personal/verter`
//!           is marker #8, not re-listed).
//!   21-26.  V6 drive scratch, backslash spellings — `D:\tmp\`, `D:\tmp/`,
//!           `D:/tmp\`, `d:\tmp\`, `d:\tmp/`, `d:/tmp\` (the all-forward
//!           `D:/tmp/` and `d:/tmp/` are markers #12/#13, not re-listed).
//!
//! Markers 27-62 close the developer's `dev/wt` (worktree-root)
//! and `dev/temp` (sandbox-scratch) sub-roots — the siblings of the V5
//! checkout root that hold the same dev's ephemeral worktrees and run
//! sandboxes. They are SCOPED to the `wt`/`temp` sub-roots, each bounded by
//! a TRAILING separator so a marker cannot substring-match an unrelated
//! path sharing the prefix text:
//!
//!   27-58.  32 drive-form spellings — drive ∈ {`D:`,`d:`} × the 3
//!           separator positions ∈ {`/`,`\`} × sub-root ∈ {`wt`,`temp`}
//!           (e.g. `D:/dev/wt/`, `D:\dev\temp\`, `d:/dev\wt/`).
//!   59-62.  4 POSIX-mount spellings — `/d/dev/wt/`, `/d/dev/temp/`,
//!           `/mnt/d/dev/wt/`, `/mnt/d/dev/temp/` (Git-Bash + WSL).
//!
//! A bare `D:\dev` marker is DELIBERATELY NOT added: it sits one component
//! above the legitimate `dev/project` / `dev/example` fixture family and
//! would false-positive it. The scoped `dev/wt` / `dev/temp` markers do not.
//!
//! Markers 63-64 are the lowercase-drive spellings (`c:/Users/david/.claude`,
//! `c:\Users\david\.claude`) of the already-classified Windows Claude
//! personal-config root #6/#7. The path/URI normalization layer canonicalizes
//! Windows drive letters to lowercase, so the lowercase `c:` twin is the same
//! machine root in the spelling the codebase actually emits. The marker still
//! requires the exact `.claude` segment, so it does NOT match the legitimate
//! generic `c:/Users/dev` / `c:/Users/david/workspace` fixtures.
//!
//! The 64 markers above MAY appear here as doc-comment literals — this
//! file is the single allowlisted self-file, matched by its EXACT
//! repo-relative path (see `SELF_FILE_REPO_PATH`, not a basename suffix),
//! because it must reference the markers to check for them. No other file is
//! exempt. The RUNTIME `MACHINE_MARKERS` const initializer is built from
//! SPLIT fragments via `concat!` (compile-time, zero runtime cost) so the
//! const's executable expression holds no contiguous marker; the
//! allowlisted self-file nonetheless DOES contain contiguous marker
//! literals — in this doc-comment and in the
//! `constructed_markers_equal_intended_bytes` set-pin — which is why this
//! one file (and only this file, by exact path) is allowlisted.
//! `constructed_markers_equal_intended_bytes` proves the fragments
//! reassemble to the intended bytes (the backslash marker #7 especially:
//! `concat!("C:\\Users", "\\david\\.claude")` reassembles to the runtime
//! string `C:\Users\david\.claude`).
//!
//! The failure message names the offending file AND the marker, so a
//! future violation is actionable.
//!
//! DOCUMENTED RESIDUAL: this guard catches only the 64 unambiguous fixed
//! markers above — it is NOT a complete machine-path detector. Markers
//! 14-26 close every separator spelling (forward, backslash, UNC, mixed) of
//! the V5 checkout root and V6 `D:`/`d:` drive scratch; markers 27-62 close
//! the same dev's scoped `dev/wt` worktree-root and `dev/temp` sandbox
//! scratch; markers 63-64 close the lowercase-drive `c:` twins of the
//! Windows Claude personal-config root #6/#7. DELIBERATELY uncaught: a NEW
//! different dev's `$HOME`, a third username, a different drive letter's
//! scratch (`E:\tmp\…`), a bare-parent `dev` root (`D:\dev` with no
//! `wt`/`temp`/`personal` tail), the SSR scratch `C:/temp/` class (fixed
//! in-files, not marker-guarded), or any other machine-local path NOT in this
//! fixed list. These are fixed in-files (or gitignored) rather than caught
//! here, because catching the home/scratch/bare-parent class broadly
//! (`/Users/`, `C:/Users/`, `/home/`, `/tmp/`, `[A-Za-z]:\tmp\`,
//! `[A-Za-z]:\dev`) would false-positive the ~70 legitimate representative
//! cross-platform path/URI fixtures the repo intentionally carries (generic
//! `c:/Users/dev`, `/Users/Foo/Bar.vue`, `/home/runner`, `C:/tmp/Foo.vue.ts`,
//! `C:\tmp\Foo.vue.ts`, `D:\dev\project`, `d:/dev/example`, Linux-CI `/tmp`).
//! The companion policy for future local tool state is to gitignore it rather
//! than widen this scanner.
//!
//! scanner_invariant: no tracked file embeds any of the 64 fixed
//!   machine/user/session/orchestration absolute-path markers; the scan is
//!   fail-closed on BOTH the file read (an unreadable tracked file is a
//!   guard failure) AND the path encoding (a non-UTF-8 tracked path is a
//!   guard failure).
//! scanner_justification: a content-residue invariant over tracked-file
//!   TEXT cannot be expressed by any compiler or structural mechanism —
//!   the offending bytes are arbitrary string literals inside source,
//!   docs, JSON fixtures, and skill files. Per Structural-Confinement-
//!   First, a structural mechanism is preferred wherever one exists; none
//!   exists here, so a fixed-marker tree scanner is the correct (recorded,
//!   justified) mechanism.
//! mechanism_ruling: fixed-marker content scanner for the Cross-Platform
//!   Portability content-residue invariant; accepted by the durable
//!   architecture-ruling record at
//!   `docs/arch/portability-fixed-marker-scanner-rulings.md#tracked-paths-no-machine-roots`.
//! hardening_rounds: 3
//! hardening_history: proof entries for hardening_rounds=3 (each a bounded
//!   marker-set expansion authorized by the durable ruling above; the exact
//!   marker inventory and `constructed_markers_equal_intended_bytes` set-pin
//!   in this file are the landed-tree evidence):
//!   - entry A: marker span 14-26 — separator-equivalent spellings
//!     (Windows-backslash / mixed-separator / UNC) of the already-classified
//!     V5 checkout-root tail and V6 `D:`/`d:` drive scratch. Same machine
//!     roots, no new class, no broad home/scratch detector, no broad
//!     `D:/dev`.
//!   - entry B: marker span 27-62 — the same developer's scoped `dev/wt`
//!     worktree-root and `dev/temp` sandbox-scratch roots across drive,
//!     separator, Git-Bash, and WSL spellings, each trailing-separator-
//!     bounded. Scoped known roots only, NOT a broad `D:/dev` ban; the
//!     discrimination negatives for `dev/project` and `dev/example` pin the
//!     boundary.
//!   - entry C: marker span 63-64 — the lowercase-drive spellings of the
//!     already-classified Windows Claude personal-config root #6/#7
//!     (`C:/Users/david/.claude`). Drive-letter case is spelling only for
//!     this root: the path/URI normalization layer canonicalizes Windows
//!     drive letters to lowercase, and the marker set already treats
//!     drive-case twins as same-family. No new username, no new directory
//!     class, no broad `c:/Users` coverage — the marker still requires the
//!     exact `.claude` segment, so it does not touch the legitimate generic
//!     Windows path/URI fixture families. A bounded case-normalization
//!     completion, authorized by a reopened Structural-Confinement
//!     architecture review recorded in the durable ruling above.
//!   - terminal_state: hardening_rounds=3 is the bound reached for this
//!     scanner. No further marker additions — same-class or otherwise — are
//!     allowed without reopening the Structural-Confinement decision through
//!     the architecture rail. A same-class future discovery is fixed in
//!     files (or ignored by git), never appended here without that ruling.

use std::path::PathBuf;
use std::process::Command;

/// The 64 fixed machine/user/session/orchestration markers, each built
/// from split fragments via `concat!` so the CONST INITIALIZER (the
/// executable const expression) holds no contiguous marker — the guard
/// therefore needs no content-allowlist of its executable body, and an
/// allowlist typo cannot mask a real hit. (The self-file's doc-comments
/// and the `constructed_markers_equal_intended_bytes` set-pin do contain
/// contiguous marker literals; that is why this one file is allowlisted by
/// exact path.)
const MACHINE_MARKERS: &[&str] = &[
    concat!("/Users/", "carlosrodrigues"),
    concat!("/private/tmp", "/claude"),
    concat!("/tmp", "/mom"),
    concat!("/tmp", "/orch"),
    concat!("/d/tmp", "/orch"),
    concat!("C:/Users/", "david/.claude"),
    concat!("C:\\Users", "\\david\\.claude"),
    concat!("/dev/", "personal/verter"),
    concat!("D:/dev/", "personal/verter"),
    concat!("d:/dev/", "personal/verter"),
    concat!("/mnt/d/dev/", "personal/verter"),
    concat!("D:/", "tmp/"),
    concat!("d:/", "tmp/"),
    // V5 checkout-root tail — every spelling containing at least one
    // backslash separator (the all-forward `/dev/personal/verter` is
    // already marker #8 above and is NOT duplicated here). Slash direction
    // is only spelling; each is the same machine-specific checkout root.
    concat!("\\dev", "\\personal\\verter"),
    concat!("\\dev", "\\personal/verter"),
    concat!("\\dev", "/personal\\verter"),
    concat!("\\dev", "/personal/verter"),
    concat!("/dev", "\\personal\\verter"),
    concat!("/dev", "\\personal/verter"),
    concat!("/dev", "/personal\\verter"),
    // V6 Windows-drive scratch — backslash spellings (the all-forward
    // `D:/tmp/` and `d:/tmp/` are already markers #12/#13 above and are NOT
    // duplicated here). Only the classified `D:`/`d:` scratch roots close.
    concat!("D:", "\\tmp\\"),
    concat!("D:", "\\tmp/"),
    concat!("D:", "/tmp\\"),
    concat!("d:", "\\tmp\\"),
    concat!("d:", "\\tmp/"),
    concat!("d:", "/tmp\\"),
    // Markers 27-62: the scoped `dev/wt` (worktree-root) and
    // `dev/temp` (sandbox-scratch) siblings of the already-classified
    // checkout root — all drive/separator/POSIX-mount spellings, each
    // TRAILING-separator-bounded so a marker cannot substring-match an
    // unrelated path that merely shares the `dev/wt`/`dev/temp` prefix
    // text. Scoped to the `wt`/`temp` sub-roots ONLY — a bare `D:\dev`
    // marker is DELIBERATELY absent (it sits one component above the
    // legitimate `dev/project|example` fixture family and would
    // false-positive it). 32 drive-form spellings: drive ∈ {`D:`,`d:`}
    // × the 3 separator positions ∈ {`/`,`\`} × sub-root ∈ {`wt`,`temp`}.
    concat!("D:", "/dev/wt/"),
    concat!("D:", "/dev/wt\\"),
    concat!("D:", "/dev\\wt/"),
    concat!("D:", "/dev\\wt\\"),
    concat!("D:", "\\dev/wt/"),
    concat!("D:", "\\dev/wt\\"),
    concat!("D:", "\\dev\\wt/"),
    concat!("D:", "\\dev\\wt\\"),
    concat!("D:", "/dev/temp/"),
    concat!("D:", "/dev/temp\\"),
    concat!("D:", "/dev\\temp/"),
    concat!("D:", "/dev\\temp\\"),
    concat!("D:", "\\dev/temp/"),
    concat!("D:", "\\dev/temp\\"),
    concat!("D:", "\\dev\\temp/"),
    concat!("D:", "\\dev\\temp\\"),
    concat!("d:", "/dev/wt/"),
    concat!("d:", "/dev/wt\\"),
    concat!("d:", "/dev\\wt/"),
    concat!("d:", "/dev\\wt\\"),
    concat!("d:", "\\dev/wt/"),
    concat!("d:", "\\dev/wt\\"),
    concat!("d:", "\\dev\\wt/"),
    concat!("d:", "\\dev\\wt\\"),
    concat!("d:", "/dev/temp/"),
    concat!("d:", "/dev/temp\\"),
    concat!("d:", "/dev\\temp/"),
    concat!("d:", "/dev\\temp\\"),
    concat!("d:", "\\dev/temp/"),
    concat!("d:", "\\dev/temp\\"),
    concat!("d:", "\\dev\\temp/"),
    concat!("d:", "\\dev\\temp\\"),
    // 4 POSIX-mount spellings (Git-Bash `/d/...`, WSL `/mnt/d/...`).
    concat!("/d", "/dev/wt/"),
    concat!("/d", "/dev/temp/"),
    concat!("/mnt/d", "/dev/wt/"),
    concat!("/mnt/d", "/dev/temp/"),
    // Lowercase-drive spellings of the already-classified Windows Claude
    // personal-config root #6/#7. The path/URI normalization layer
    // canonicalizes Windows drive letters to lowercase, so the lowercase
    // `c:` twin of #6/#7 is the same machine root in the spelling the
    // codebase actually emits. The marker still requires the exact
    // `.claude` segment, so it does NOT match the legitimate generic
    // `c:/Users/dev` / `c:/Users/david/workspace` fixtures.
    concat!("c:/Users/", "david/.claude"),
    concat!("c:\\Users", "\\david\\.claude"),
];

/// The EXACT repo-relative path of THIS guard's own source file — the
/// single allowlisted file, since it must reference the markers to check
/// for them. Compared by exact equality (not basename suffix) so a future
/// tracked file that merely shares this basename elsewhere in the tree is
/// NOT wrongly skipped. `tracked_paths()` runs `git ls-files` from the
/// repo root, which emits forward-slash repo-relative paths on every OS.
const SELF_FILE_REPO_PATH: &str =
    "crates/verter_session/tests/cases/tracked_paths_no_machine_roots.rs";

/// The single "do these RAW BYTES contain a marker" predicate — the one
/// the TREE SCAN uses. Searches the file's raw bytes for each marker's
/// bytes as a contiguous sub-slice, so a marker embedded in an otherwise
/// non-UTF-8 / binary blob is still caught (every marker is pure ASCII).
/// Returns the FIRST marker found, or `None`.
fn machine_marker_hit_bytes(content: &[u8]) -> Option<&'static str> {
    MACHINE_MARKERS.iter().copied().find(|marker| {
        let needle = marker.as_bytes();
        // An empty marker would match everything; the set has none, but be
        // explicit so a future empty literal cannot silently match.
        !needle.is_empty() && content.windows(needle.len()).any(|w| w == needle)
    })
}

/// The `&str` twin of [`machine_marker_hit_bytes`], used by the
/// discrimination self-test on `&str` samples. Delegates to the SAME byte
/// core (via `str::as_bytes`) so the self-test exercises the production
/// search, not a parallel reimplementation. Returns the FIRST marker
/// found, or `None`.
fn machine_marker_hit(content: &str) -> Option<&'static str> {
    machine_marker_hit_bytes(content.as_bytes())
}

/// Resolve the repository root from the crate's manifest dir, so the
/// guard works from any worktree location (mirrors the sibling guard).
fn repo_root() -> PathBuf {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run `git rev-parse --show-toplevel`");
    assert!(
        out.status.success(),
        "git rev-parse --show-toplevel failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    PathBuf::from(
        String::from_utf8(out.stdout)
            .expect("repo root is UTF-8")
            .trim_end(),
    )
}

/// Enumerate every tracked path as a UTF-8 string. `-z` is mandatory: the
/// newline-separated form octal-quotes non-ASCII paths. The enumeration is
/// FAIL-CLOSED on path encoding: a tracked path that is not valid UTF-8 is a
/// guard failure (not a silent drop), matching this scanner's own fail-closed
/// claim and the sibling `tracked_paths_are_portable`, which also fails the
/// whole repo on a non-UTF-8 tracked path.
fn tracked_paths() -> Vec<String> {
    let out = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(repo_root())
        .output()
        .expect("run `git ls-files -z`");
    assert!(
        out.status.success(),
        "git ls-files -z failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut paths: Vec<String> = Vec::new();
    let mut decode_failures: Vec<String> = Vec::new();
    for raw in out.stdout.split(|&b| b == 0).filter(|p| !p.is_empty()) {
        match std::str::from_utf8(raw) {
            Ok(s) => paths.push(s.to_string()),
            Err(_) => decode_failures.push(String::from_utf8_lossy(raw).into_owned()),
        }
    }
    // FAIL-CLOSED on encoding: a non-UTF-8 tracked path must not be silently
    // skipped, or the marker scan could pass without examining the offending
    // path's file. The sibling path-shape guard already fails the repo on a
    // non-UTF-8 path, so this is unreachable on a passing tree — but the guard
    // must match its own fail-closed claim rather than depending on a sibling.
    assert!(
        decode_failures.is_empty(),
        "{} tracked path(s) are not valid UTF-8 — the marker scan must examine \
         every tracked path (fail-closed on encoding); a non-UTF-8 tracked path \
         is itself a guard failure:\n  {}",
        decode_failures.len(),
        decode_failures.join("\n  ")
    );
    paths
}

#[test]
fn tracked_files_contain_no_machine_specific_path_markers() {
    let root = repo_root();
    let paths = tracked_paths();
    assert!(
        paths.len() > 1000,
        "suspiciously few tracked paths ({}) — enumeration is broken",
        paths.len()
    );

    let mut violations: Vec<String> = Vec::new();
    let mut read_errors: Vec<String> = Vec::new();

    for rel in &paths {
        // Allowlist exactly one file by its EXACT repo-relative path: this
        // guard's own source. `git ls-files` emits forward slashes on all
        // platforms; the `replace` is harmless insurance.
        if rel.replace('\\', "/") == SELF_FILE_REPO_PATH {
            continue;
        }

        let abs = root.join(rel);
        // FAIL-CLOSED: a tracked file that cannot be read is itself a guard
        // failure, NOT a silent skip. Skipping on read error fails OPEN — the
        // assertion could pass without ever scanning the offending file. The
        // scan reads WORKTREE bytes (not git blobs) so it also catches dirty
        // tracked files; in the CI clean-checkout scenario every tracked file
        // is readable, so requiring readability is correct. The scan operates
        // on RAW BYTES — a non-UTF-8 / binary blob is NOT skipped, because the
        // markers are pure ASCII and can appear verbatim inside one.
        let bytes = match std::fs::read(&abs) {
            Ok(b) => b,
            Err(e) => {
                read_errors.push(format!("{rel}: {e}"));
                continue;
            }
        };

        if let Some(marker) = machine_marker_hit_bytes(&bytes) {
            violations.push(format!(
                "{rel}: contains machine-specific marker `{marker}`"
            ));
        }
    }

    // Fail-closed gate: every tracked file must have been readable and
    // scanned. A deleted-but-tracked or otherwise-unreadable path fails here
    // rather than slipping past the marker scan unexamined.
    assert!(
        read_errors.is_empty(),
        "{} tracked file(s) could not be read — the marker scan must examine \
         every tracked file (fail-closed); a deleted/unreadable tracked path \
         is itself a guard failure:\n  {}",
        read_errors.len(),
        read_errors.join("\n  ")
    );

    assert!(
        violations.is_empty(),
        "tracked files must not embed machine/user/session/orchestration \
         absolute-path markers (use std::env::temp_dir() / os.tmpdir() / \
         env-driven / repo-relative paths instead); {} violation(s):\n  {}",
        violations.len(),
        violations.join("\n  ")
    );
}

#[test]
fn constructed_markers_equal_intended_bytes() {
    // Prove the `concat!` fragments reassemble to the intended runtime
    // strings — especially the backslash marker #7, where each `\\` in
    // source is one backslash at runtime.
    assert_eq!(MACHINE_MARKERS.len(), 64, "the marker set is exactly 64");

    // Pin the EXACT intended marker SET (not just len): a DROPPED marker
    // makes this inequality fail even though the self-matching positive
    // loop in `machine_marker_matcher_discriminates` would still pass (it
    // only checks markers that are PRESENT). Each `\\` here is one runtime
    // backslash, so the listed literals are the reassembled `concat!`
    // values verbatim. This file is the allowlisted self-file, so the
    // contiguous markers may appear in this assertion.
    assert_eq!(
        MACHINE_MARKERS,
        &[
            "/Users/carlosrodrigues",
            "/private/tmp/claude",
            "/tmp/mom",
            "/tmp/orch",
            "/d/tmp/orch",
            "C:/Users/david/.claude",
            "C:\\Users\\david\\.claude",
            "/dev/personal/verter",
            "D:/dev/personal/verter",
            "d:/dev/personal/verter",
            "/mnt/d/dev/personal/verter",
            "D:/tmp/",
            "d:/tmp/",
            "\\dev\\personal\\verter",
            "\\dev\\personal/verter",
            "\\dev/personal\\verter",
            "\\dev/personal/verter",
            "/dev\\personal\\verter",
            "/dev\\personal/verter",
            "/dev/personal\\verter",
            "D:\\tmp\\",
            "D:\\tmp/",
            "D:/tmp\\",
            "d:\\tmp\\",
            "d:\\tmp/",
            "d:/tmp\\",
            // Markers 27-62: scoped `dev/wt` + `dev/temp` roots,
            // all drive/separator/POSIX-mount spellings, trailing-separator.
            "D:/dev/wt/",
            "D:/dev/wt\\",
            "D:/dev\\wt/",
            "D:/dev\\wt\\",
            "D:\\dev/wt/",
            "D:\\dev/wt\\",
            "D:\\dev\\wt/",
            "D:\\dev\\wt\\",
            "D:/dev/temp/",
            "D:/dev/temp\\",
            "D:/dev\\temp/",
            "D:/dev\\temp\\",
            "D:\\dev/temp/",
            "D:\\dev/temp\\",
            "D:\\dev\\temp/",
            "D:\\dev\\temp\\",
            "d:/dev/wt/",
            "d:/dev/wt\\",
            "d:/dev\\wt/",
            "d:/dev\\wt\\",
            "d:\\dev/wt/",
            "d:\\dev/wt\\",
            "d:\\dev\\wt/",
            "d:\\dev\\wt\\",
            "d:/dev/temp/",
            "d:/dev/temp\\",
            "d:/dev\\temp/",
            "d:/dev\\temp\\",
            "d:\\dev/temp/",
            "d:\\dev/temp\\",
            "d:\\dev\\temp/",
            "d:\\dev\\temp\\",
            "/d/dev/wt/",
            "/d/dev/temp/",
            "/mnt/d/dev/wt/",
            "/mnt/d/dev/temp/",
            // Markers 63-64: lowercase-drive `c:` twins of #6/#7, the
            // already-classified Windows Claude personal-config root.
            "c:/Users/david/.claude",
            "c:\\Users\\david\\.claude",
        ],
        "the intended 64-marker SET — a dropped or typo'd marker fails here"
    );

    assert_eq!(MACHINE_MARKERS[5], "C:/Users/david/.claude");
    // Marker #7 is the backslash twin of #6: `C:\Users\david\.claude`.
    // It carries exactly 3 backslashes (before `Users`, `david`, and
    // `.claude`) and zero forward slashes — proving the `concat!`
    // fragments (`"C:\\Users"` + `"\\david\\.claude"`) reassemble to one
    // backslash per `\\`.
    let backslash = MACHINE_MARKERS[6];
    assert_eq!(backslash, "C:\\Users\\david\\.claude");
    assert_eq!(backslash.matches('\\').count(), 3);
    assert_eq!(backslash.matches('/').count(), 0);
    assert_eq!(backslash.as_bytes()[0], b'C');
    assert_eq!(backslash.as_bytes()[1], b':');
    assert_eq!(backslash.as_bytes()[2], b'\\');
    assert_eq!(backslash.matches("david").count(), 1);
    assert_eq!(backslash.matches(".claude").count(), 1);

    // Markers 63-64 are the lowercase-drive twins of #6/#7. The backslash
    // form `c:\Users\david\.claude` carries exactly 3 backslashes and zero
    // forward slashes, proving `concat!("c:\\Users", "\\david\\.claude")`
    // reassembles to one backslash per `\\` with a lowercase drive.
    assert_eq!(MACHINE_MARKERS[62], "c:/Users/david/.claude");
    let lc_back = MACHINE_MARKERS[63];
    assert_eq!(lc_back, "c:\\Users\\david\\.claude");
    assert_eq!(lc_back.matches('\\').count(), 3);
    assert_eq!(lc_back.matches('/').count(), 0);
    assert_eq!(lc_back.as_bytes()[0], b'c');
    assert_eq!(lc_back.as_bytes()[1], b':');
    assert_eq!(lc_back.matches(".claude").count(), 1);

    // NEW V5 backslash markers — representative reassembly proofs.
    // Index 13 is the all-backslash checkout-root tail `\dev\personal\verter`:
    // exactly 3 backslashes (before `dev`, `personal`, `verter`) and zero
    // forward slashes, proving `concat!("\\dev", "\\personal\\verter")`
    // reassembles to one backslash per `\\`.
    let v5_all_back = MACHINE_MARKERS[13];
    assert_eq!(v5_all_back, "\\dev\\personal\\verter");
    assert_eq!(v5_all_back.matches('\\').count(), 3);
    assert_eq!(v5_all_back.matches('/').count(), 0);
    assert_eq!(v5_all_back.as_bytes()[0], b'\\');
    assert_eq!(v5_all_back.matches("personal").count(), 1);
    assert_eq!(v5_all_back.matches("verter").count(), 1);
    // Index 14 is a MIXED-separator twin `\dev\personal/verter`: exactly 2
    // backslashes and 1 forward slash — proving the fragments preserve mixed
    // separators, not normalize them.
    let v5_mixed = MACHINE_MARKERS[14];
    assert_eq!(v5_mixed, "\\dev\\personal/verter");
    assert_eq!(v5_mixed.matches('\\').count(), 2);
    assert_eq!(v5_mixed.matches('/').count(), 1);

    // NEW V6 drive-scratch backslash marker — representative reassembly.
    // Index 20 is `D:\tmp\`: exactly 2 backslashes (before `tmp` and the
    // trailing separator) and zero forward slashes, proving
    // `concat!("D:", "\\tmp\\")` reassembles to one backslash per `\\`.
    let v6_back = MACHINE_MARKERS[20];
    assert_eq!(v6_back, "D:\\tmp\\");
    assert_eq!(v6_back.matches('\\').count(), 2);
    assert_eq!(v6_back.matches('/').count(), 0);
    assert_eq!(v6_back.as_bytes()[0], b'D');
    assert_eq!(v6_back.as_bytes()[1], b':');
    assert_eq!(v6_back.as_bytes()[2], b'\\');

    // Scoped `dev/wt` + `dev/temp` markers (indices 26-61) —
    // representative reassembly proofs across the spelling axes, proving the
    // `concat!("D:", "/dev/wt/")`-style fragments reassemble with no slash
    // normalization and a TRAILING separator.
    //
    // Index 26 — all-forward wt root `D:/dev/wt/`: 3 forward slashes (after
    // `D:`, after `dev`, trailing), zero backslashes, trailing `/`.
    let wt_fwd = MACHINE_MARKERS[26];
    assert_eq!(wt_fwd, "D:/dev/wt/");
    assert_eq!(wt_fwd.matches('/').count(), 3);
    assert_eq!(wt_fwd.matches('\\').count(), 0);
    assert!(wt_fwd.ends_with('/'));
    assert_eq!(wt_fwd.matches("wt").count(), 1);
    // Index 33 — all-backslash wt root `D:\dev\wt\`: 3 backslashes, zero
    // forward slashes, trailing `\`.
    let wt_back = MACHINE_MARKERS[33];
    assert_eq!(wt_back, "D:\\dev\\wt\\");
    assert_eq!(wt_back.matches('\\').count(), 3);
    assert_eq!(wt_back.matches('/').count(), 0);
    assert!(wt_back.ends_with('\\'));
    // Index 28 — mixed-separator wt root `D:/dev\wt/`: 2 forward slashes,
    // 1 backslash — proving the fragments preserve mixed separators.
    let wt_mixed = MACHINE_MARKERS[28];
    assert_eq!(wt_mixed, "D:/dev\\wt/");
    assert_eq!(wt_mixed.matches('/').count(), 2);
    assert_eq!(wt_mixed.matches('\\').count(), 1);
    // Index 34 — all-forward temp root `D:/dev/temp/`: the `temp` sub-root
    // twin of index 26, 3 forward slashes, trailing `/`.
    let temp_fwd = MACHINE_MARKERS[34];
    assert_eq!(temp_fwd, "D:/dev/temp/");
    assert_eq!(temp_fwd.matches('/').count(), 3);
    assert_eq!(temp_fwd.matches('\\').count(), 0);
    assert_eq!(temp_fwd.matches("temp").count(), 1);
    // Index 58 — POSIX-mount wt root `/d/dev/wt/`: pure-forward Git-Bash
    // drive-mount spelling, 4 forward slashes (leading, after `d`, after
    // `dev`, trailing), leading and trailing `/`.
    let posix_wt = MACHINE_MARKERS[58];
    assert_eq!(posix_wt, "/d/dev/wt/");
    assert_eq!(posix_wt.matches('/').count(), 4);
    assert_eq!(posix_wt.matches('\\').count(), 0);
    assert!(posix_wt.starts_with('/'));
    assert!(posix_wt.ends_with('/'));
    // Index 61 — WSL-mount temp root `/mnt/d/dev/temp/`.
    let posix_mnt_temp = MACHINE_MARKERS[61];
    assert_eq!(posix_mnt_temp, "/mnt/d/dev/temp/");
    assert_eq!(posix_mnt_temp.matches("temp").count(), 1);
}

#[test]
fn machine_marker_matcher_discriminates() {
    // POSITIVE: each of the 64 markers is detected when embedded in
    // surrounding text — this exercises the SAME production predicate the
    // tree scan uses. Some markers are substrings of others (`/tmp/orch`
    // ⊂ `/d/tmp/orch`; `/dev/personal/verter` ⊂ its drive-prefixed
    // twins; the backslash V5 tails ⊂ their `D:`/`d:`/UNC-prefixed
    // spellings), so the matcher may report a DIFFERENT-but-equally-valid
    // marker than the one planted — any hit is a real violation, so the
    // discrimination proof is "Some for every positive, None for every
    // negative", not exact-marker equality.
    for marker in MACHINE_MARKERS {
        let planted = format!("prefix text {marker} suffix text");
        assert!(
            machine_marker_hit(&planted).is_some(),
            "planted positive for marker `{marker}` must be detected"
        );
    }

    // NEGATIVE controls: representatives of the ~70 legitimate L1/L2
    // cross-platform path/URI fixtures the repo intentionally carries.
    // These MUST stay clean — a hit here means the matcher is too broad.
    let negatives = [
        "c:/Users/dev/foo",
        "/Users/Foo/Bar.vue",
        "/home/user/x",
        "/home/runner/work",
        "C:/tmp/Foo_abc.vue.ts",
        // The legitimate "david workspace" canonicalization fixture: marker
        // #6 is `C:/Users/david/.claude` (with `.claude`).
        // `C:/Users/david/workspace` has no `.claude`, so it must NOT match.
        "C:/Users/david/workspace/x",
        "C:/Users/david/node_modules/y",
        "/tmp/nuxt-ui",
        "/tmp/p1.txt",
        "/tmp/stack.txt",
        // A legit Windows checker-fixture scratch spelling. Only `D:\tmp\`
        // and `d:\tmp\` (and their separator twins) are banned drive-scratch
        // roots — `C:\tmp\` is a different drive and stays clean. Guards the
        // new V6 backslash markers against over-broad `[A-Za-z]:\tmp\`.
        "C:\\tmp\\Foo.vue.ts",
        // A legit generic Windows workspace fixture. `C:\Users\dev\workspace`
        // contains neither `\dev\personal\verter` (no `personal`) nor any
        // `D:`/`d:` scratch root — it must stay clean. Guards the new V5
        // backslash markers against matching a bare `\dev` component.
        "C:\\Users\\dev\\workspace\\x",
        // Scoped `dev/wt`/`dev/temp` discrimination: the legitimate
        // `dev/project` and `dev/example` fixtures contain `dev` but NEITHER
        // `dev/wt` NOR `dev/temp`, so the 36 scoped markers must NOT fire
        // on them. This is the boundary the scoped-root markers were kept
        // narrow to respect — a bare `D:\dev` marker would wrongly flag these.
        "D:\\dev\\project\\x",
        "D:/dev/project/x",
        "/d/dev/project/x",
        "d:/dev/example/y",
        // Near-miss: `dev/website` shares the `dev/w…` prefix with `dev/wt`
        // but the trailing-separator bound (`wt/` vs `we`) keeps it clean.
        "D:/dev/website/src",
    ];
    for control in negatives {
        assert_eq!(
            machine_marker_hit(control),
            None,
            "legitimate fixture `{control}` must NOT be flagged"
        );
    }

    // The lowercase-drive Claude personal-config twin (markers #63/#64) must
    // be caught — the codebase canonicalizes drive letters to lowercase.
    assert!(
        machine_marker_hit("x c:/Users/david/.claude/plans/y").is_some(),
        "lowercase-drive Claude config path must be detected (marker #63)"
    );
    assert!(
        machine_marker_hit("x c:\\Users\\david\\.claude\\plans y").is_some(),
        "lowercase-drive backslash Claude config path must be detected (marker #64)"
    );
    // ...but the legitimate lowercase generic workspace fixture (no `.claude`)
    // must STAY clean — proving the twin did not broaden to `c:/Users`.
    assert_eq!(machine_marker_hit("c:/Users/david/workspace/x"), None);

    // BYTE-SCAN discrimination: a marker's ASCII bytes embedded inside an
    // otherwise NON-UTF-8 blob must be caught. This case FAILs against a
    // pre-fix lossy-only scan (which `continue`d on `from_utf8` error and
    // never looked at the bytes) and PASSes against the byte-scan fix.
    let marker = MACHINE_MARKERS[0]; // `/Users/carlosrodrigues`, pure ASCII
    let mut blob: Vec<u8> = vec![0xFF, 0xFE]; // invalid UTF-8 lead bytes
    blob.extend_from_slice(marker.as_bytes());
    blob.extend_from_slice(b"\x00trailing");
    // The OLD lossy-UTF-8 gate would have skipped this file outright...
    assert!(
        std::str::from_utf8(&blob).is_err(),
        "the discrimination blob must be invalid UTF-8, so the pre-fix \
         `from_utf8`+continue path would have skipped it"
    );
    // ...but the byte predicate (the one the tree scan now uses) catches it.
    assert_eq!(
        machine_marker_hit_bytes(&blob),
        Some(marker),
        "byte-scan must catch an ASCII marker embedded in non-UTF-8 bytes"
    );
}
