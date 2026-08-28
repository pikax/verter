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
//! file is the intrinsic self-file, matched by its EXACT repo-relative path
//! (see `SELF_FILE_REPO_PATH`, not a basename suffix), because it must
//! reference the markers to check for them. Exact authority evidence is not
//! skipped: every file is read and scanned, and a real hit is admitted only
//! after the separately ratified manifest validates its exact path, worktree
//! SHA-256, existing pin document, permitted root, and liveness. The RUNTIME
//! `MACHINE_MARKERS` const initializer is built from
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
//!   `docs/contributing/portability-fixed-marker-scanner-rulings.md#tracked-paths-no-machine-roots`.
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

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

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

/// The EXACT repo-relative path of THIS guard's own source file. It is scanned
/// like every other tracked file, then intrinsically admitted because it must
/// spell the marker set it enforces. Compared by exact equality (not basename
/// suffix) so a future tracked file that merely shares this basename elsewhere
/// in the tree is NOT admitted. `tracked_paths()` runs `git ls-files` from the
/// repo root, which emits forward-slash repo-relative paths on every OS.
const SELF_FILE_REPO_PATH: &str =
    "crates/verter_source_policy_gate/tests/cases/tracked_paths_no_machine_roots.rs";
const EVIDENCE_EXCEPTION_MANIFEST_REPO_PATH: &str =
    "scripts/manifests/portability-machine-marker-evidence-exceptions.tsv";
const SOURCE_POLICY_RULING_REPO_PATH: &str =
    "docs/contributing/portability-fixed-marker-scanner-rulings.md";
const EVIDENCE_EXCEPTION_MANIFEST_HEADER: &str =
    "class\tpath\tsha256\tpin_document\towner\tretirement_gate";
const AUTHORITY_EVIDENCE_ROOT: &str = "docs/arch/refactor/rev11/backup/evidence/";
const ARCHITECTURE_RULING_ROOT: &str = "docs/arch/refactor/rev11/backup/rulings/";
const HISTORICAL_REVIEW_SOURCE_ROOT: &str =
    "docs/arch/refactor/rev11/sources/review-history-migration/";

#[derive(Debug, Clone)]
struct EvidenceException {
    path: String,
    sha256: String,
}

#[derive(Debug, Default)]
struct MarkerScanReport {
    violations: Vec<String>,
    read_errors: Vec<String>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

fn is_lower_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn is_exact_repo_path(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('\\')
        && !value
            .bytes()
            .any(|byte| matches!(byte, b'*' | b'?' | b'[' | b']'))
        && Path::new(value)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn source_policy_manifest_digest(ruling: &[u8]) -> Result<String, String> {
    let ruling =
        std::str::from_utf8(ruling).map_err(|_| "source-policy ruling is not UTF-8".to_string())?;
    let prefix = "Exception manifest SHA-256: `";
    let mut matches = ruling.match_indices(prefix);
    let Some((offset, _)) = matches.next() else {
        return Err(
            "source-policy ruling is missing the exception manifest digest pin".to_string(),
        );
    };
    if matches.next().is_some() {
        return Err(
            "source-policy ruling contains duplicate exception manifest digest pins".to_string(),
        );
    }
    let digest_start = offset + prefix.len();
    let Some(rest) = ruling.get(digest_start..) else {
        return Err(
            "source-policy ruling has a truncated exception manifest digest pin".to_string(),
        );
    };
    let Some(end) = rest.find('`') else {
        return Err(
            "source-policy ruling has an unterminated exception manifest digest pin".to_string(),
        );
    };
    let digest = &rest[..end];
    if !is_lower_sha256(digest) {
        return Err(format!(
            "source-policy ruling has malformed exception manifest digest `{digest}`"
        ));
    }
    Ok(digest.to_string())
}

fn validate_evidence_exception_manifest(
    root: &Path,
    tracked_paths: &[String],
    manifest_repo_path: &str,
    source_policy_ruling_repo_path: &str,
    expected_rows: usize,
) -> Result<BTreeMap<String, EvidenceException>, Vec<String>> {
    let mut errors = Vec::new();
    let tracked: HashSet<&str> = tracked_paths.iter().map(String::as_str).collect();
    let manifest = match std::fs::read(root.join(manifest_repo_path)) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Err(vec![format!(
                "missing exception manifest `{manifest_repo_path}`: {error}"
            )]);
        }
    };
    let ruling = match std::fs::read(root.join(source_policy_ruling_repo_path)) {
        Ok(bytes) => bytes,
        Err(error) => {
            return Err(vec![format!(
                "missing source-policy ruling `{source_policy_ruling_repo_path}`: {error}"
            )]);
        }
    };
    match source_policy_manifest_digest(&ruling) {
        Ok(expected_digest) => {
            let actual_digest = sha256_hex(&manifest);
            if actual_digest != expected_digest {
                errors.push(format!(
                    "exception manifest digest mismatch: ruling pins {expected_digest}, worktree bytes are {actual_digest}"
                ));
            }
        }
        Err(error) => errors.push(error),
    }

    let manifest_text = match std::str::from_utf8(&manifest) {
        Ok(text) => text,
        Err(_) => {
            errors.push("exception manifest is not UTF-8".to_string());
            return Err(errors);
        }
    };
    if manifest_text.contains('\r') {
        errors.push("exception manifest must use LF line endings".to_string());
    }
    let mut lines = manifest_text.lines();
    let header = lines.next().unwrap_or_default();
    if header != EVIDENCE_EXCEPTION_MANIFEST_HEADER {
        errors.push(format!(
            "exception manifest header must be exactly `{EVIDENCE_EXCEPTION_MANIFEST_HEADER}`"
        ));
    }

    let rows: Vec<&str> = lines.collect();
    if rows.len() != expected_rows {
        errors.push(format!(
            "exception manifest must contain exactly {expected_rows} data rows, found {}",
            rows.len()
        ));
    }

    let mut exceptions = BTreeMap::new();
    for (index, line) in rows.iter().enumerate() {
        let line_number = index + 2;
        let columns: Vec<&str> = line.split('\t').collect();
        if columns.len() != 6 || columns.iter().any(|column| column.is_empty()) {
            errors.push(format!(
                "exception manifest line {line_number} must contain six non-empty tab-separated columns"
            ));
            continue;
        }
        let [class, path, digest, pin_document, _owner, _retirement_gate] = columns.as_slice()
        else {
            unreachable!("column count checked above")
        };
        if !is_exact_repo_path(path) {
            let kind = if path
                .bytes()
                .any(|byte| matches!(byte, b'*' | b'?' | b'[' | b']'))
            {
                "wildcard"
            } else {
                "non-exact"
            };
            errors.push(format!(
                "exception manifest line {line_number} has {kind} path `{path}`"
            ));
        }
        let permitted = match *class {
            "authority-evidence" => path.starts_with(AUTHORITY_EVIDENCE_ROOT),
            "inherited-ruling" => path.starts_with(ARCHITECTURE_RULING_ROOT),
            "historical-review-source" => path.starts_with(HISTORICAL_REVIEW_SOURCE_ROOT),
            _ => {
                errors.push(format!(
                    "exception manifest line {line_number} has unknown class `{class}`"
                ));
                false
            }
        };
        if !permitted {
            errors.push(format!(
                "exception manifest line {line_number} path `{path}` is outside the permitted roots for class `{class}`"
            ));
        }
        if !is_lower_sha256(digest) {
            errors.push(format!(
                "exception manifest line {line_number} has malformed digest `{digest}`"
            ));
        }
        if !is_exact_repo_path(pin_document) {
            errors.push(format!(
                "exception manifest line {line_number} has non-exact pin document `{pin_document}`"
            ));
        }
        if !tracked.contains(path) {
            errors.push(format!(
                "stale exception row `{path}` is not a tracked file"
            ));
        }
        if !tracked.contains(pin_document) || !root.join(pin_document).is_file() {
            errors.push(format!(
                "missing pin document `{pin_document}` for exception `{path}`"
            ));
        }
        if exceptions.contains_key(*path) {
            errors.push(format!("duplicate exception path `{path}`"));
            continue;
        }

        let bytes = match std::fs::read(root.join(path)) {
            Ok(bytes) => bytes,
            Err(error) => {
                errors.push(format!("cannot read exception path `{path}`: {error}"));
                continue;
            }
        };
        if is_lower_sha256(digest) {
            let actual_digest = sha256_hex(&bytes);
            if actual_digest != *digest {
                errors.push(format!(
                    "exception `{path}` digest mismatch: manifest pins {digest}, worktree bytes are {actual_digest}"
                ));
            }
        }
        if machine_marker_hit_bytes(&bytes).is_none() {
            errors.push(format!(
                "stale exception row `{path}` remains after its machine marker was removed"
            ));
        }
        if root.join(pin_document).is_file() && is_lower_sha256(digest) {
            match std::fs::read(root.join(pin_document)) {
                Ok(pin_bytes)
                    if pin_bytes
                        .windows(digest.len())
                        .any(|window| window == digest.as_bytes()) => {}
                Ok(_) => errors.push(format!(
                    "pin document `{pin_document}` does not contain digest {digest} for `{path}`"
                )),
                Err(error) => errors.push(format!(
                    "cannot read pin document `{pin_document}` for `{path}`: {error}"
                )),
            }
        }
        exceptions.insert(
            (*path).to_string(),
            EvidenceException {
                path: (*path).to_string(),
                sha256: (*digest).to_string(),
            },
        );
    }

    if errors.is_empty() {
        Ok(exceptions)
    } else {
        Err(errors)
    }
}

fn scan_marker_violations(
    root: &Path,
    paths: &[String],
    exceptions: &BTreeMap<String, EvidenceException>,
) -> MarkerScanReport {
    let mut report = MarkerScanReport::default();
    for rel in paths {
        let bytes = match std::fs::read(root.join(rel)) {
            Ok(bytes) => bytes,
            Err(error) => {
                report.read_errors.push(format!("{rel}: {error}"));
                continue;
            }
        };
        if let Some(marker) = machine_marker_hit_bytes(&bytes) {
            let admitted = rel == SELF_FILE_REPO_PATH
                || exceptions.get(rel).is_some_and(|exception| {
                    exception.path == *rel && exception.sha256 == sha256_hex(&bytes)
                });
            if !admitted {
                report.violations.push(format!(
                    "{rel}: contains machine-specific marker `{marker}`"
                ));
            }
        }
    }
    report
}

/// Per-first-byte candidate lists over [`MACHINE_MARKERS`]: `starts[b]` holds
/// the marker indices whose first byte is `b`, ascending. Built once per
/// process.
///
/// A zero-length marker has no first byte, so it is indexed nowhere and can
/// never match — the same explicit guarantee the previous `!needle.is_empty()`
/// check gave, now structural rather than a per-comparison test.
fn marker_starts() -> &'static [Vec<u16>; 256] {
    static STARTS: OnceLock<[Vec<u16>; 256]> = OnceLock::new();
    STARTS.get_or_init(|| {
        let mut starts: [Vec<u16>; 256] = std::array::from_fn(|_| Vec::new());
        for (idx, marker) in MACHINE_MARKERS.iter().enumerate() {
            if let Some(&first) = marker.as_bytes().first() {
                starts[first as usize].push(idx as u16);
            }
        }
        starts
    })
}

/// The single "do these RAW BYTES contain a marker" predicate — the one
/// the TREE SCAN uses. Searches the file's raw bytes for each marker's
/// bytes as a contiguous sub-slice, so a marker embedded in an otherwise
/// non-UTF-8 / binary blob is still caught (every marker is pure ASCII).
/// Returns the FIRST marker found, or `None`.
///
/// "First" is by [`MACHINE_MARKERS`] index, not by position in `content` —
/// unchanged from the original `iter().find(...)` shape, and asserted by
/// `machine_marker_first_hit_is_lowest_indexed_marker_not_earliest_position`.
///
/// ONE pass over `content` with a first-byte prefilter, rather than one full
/// `windows()` pass per marker. The per-marker shape ran all
/// `MACHINE_MARKERS.len()` passes to completion on every clean file (`find`
/// short-circuits only on a hit, and the tree is clean by construction), which
/// is ~6.6e9 window comparisons across ~98 MB of tracked content — enough to
/// push this guard past the CI per-test timeout on a loaded 4-core runner.
fn machine_marker_hit_bytes(content: &[u8]) -> Option<&'static str> {
    let starts = marker_starts();
    // The lowest MACHINE_MARKERS index seen so far. The scan continues past a
    // hit because a lower-indexed marker may still appear later in `content`,
    // which is what keeps the return value identical to the per-marker shape.
    let mut best: Option<usize> = None;
    for (pos, &byte) in content.iter().enumerate() {
        let candidates = &starts[byte as usize];
        if candidates.is_empty() {
            continue;
        }
        let rest = &content[pos..];
        for &candidate in candidates {
            let candidate = candidate as usize;
            // Already holding an equal-or-better (lower) index.
            if best.is_some_and(|found| found <= candidate) {
                continue;
            }
            let needle = MACHINE_MARKERS[candidate].as_bytes();
            if rest.starts_with(needle) {
                if candidate == 0 {
                    // Index 0 is minimal; nothing later can beat it.
                    return Some(MACHINE_MARKERS[0]);
                }
                best = Some(candidate);
            }
        }
    }
    best.map(|found| MACHINE_MARKERS[found])
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

    let exceptions = validate_evidence_exception_manifest(
        &root,
        &paths,
        EVIDENCE_EXCEPTION_MANIFEST_REPO_PATH,
        SOURCE_POLICY_RULING_REPO_PATH,
        15,
    );
    let authority_errors = exceptions.as_ref().err().cloned().unwrap_or_default();
    let empty = BTreeMap::new();
    let exceptions = exceptions.as_ref().unwrap_or(&empty);
    // Every tracked file is still read and scanned, including this guard and
    // every admitted authority artifact. Admission is decided only after a
    // real marker hit and only by exact self-path or the validated path+digest
    // map; there is no directory, suffix, glob, or "docs" bypass.
    let report = scan_marker_violations(&root, &paths, exceptions);

    // Fail-closed gate: every tracked file must have been readable and
    // scanned. A deleted-but-tracked or otherwise-unreadable path fails here
    // rather than slipping past the marker scan unexamined.
    assert!(
        report.read_errors.is_empty(),
        "{} tracked file(s) could not be read — the marker scan must examine \
         every tracked file (fail-closed); a deleted/unreadable tracked path \
         is itself a guard failure:\n  {}",
        report.read_errors.len(),
        report.read_errors.join("\n  ")
    );

    assert!(
        authority_errors.is_empty(),
        "machine-marker authority-evidence exception manifest is invalid; {} error(s):\n  {}",
        authority_errors.len(),
        authority_errors.join("\n  ")
    );

    assert!(
        report.violations.is_empty(),
        "tracked files must not embed machine/user/session/orchestration \
         absolute-path markers (use std::env::temp_dir() / os.tmpdir() / \
         env-driven / repo-relative paths instead); {} violation(s):\n  {}",
        report.violations.len(),
        report.violations.join("\n  ")
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

/// The matcher reports the lowest-indexed [`MACHINE_MARKERS`] entry present in
/// the content, NOT the one that happens to occur earliest in the bytes.
///
/// This pins the return-value contract the single-pass prefilter scan has to
/// preserve. It is the discriminating check against the obvious "faster"
/// rewrites: a leftmost-first multi-pattern searcher (e.g. Aho-Corasick) or any
/// scan that returns on its first positional hit answers with the LATER-indexed
/// marker here, because that marker is planted FIRST in the haystack.
#[test]
fn machine_marker_first_hit_is_lowest_indexed_marker_not_earliest_position() {
    // Two markers that are not substrings of one another, planted with the
    // HIGHER-indexed one first in the bytes.
    let (low_idx, high_idx) = (0usize, MACHINE_MARKERS.len() - 1);
    let low = MACHINE_MARKERS[low_idx];
    let high = MACHINE_MARKERS[high_idx];
    assert!(
        !low.contains(high) && !high.contains(low),
        "fixture needs two markers that do not contain each other: `{low}` / `{high}`"
    );

    let high_first = format!("lead {high} middle {low} tail");
    assert_eq!(
        machine_marker_hit(&high_first),
        Some(low),
        "the lowest-indexed marker wins even when planted later in the bytes"
    );

    // Order in the haystack must not change the answer.
    let low_first = format!("lead {low} middle {high} tail");
    assert_eq!(
        machine_marker_hit(&low_first),
        Some(low),
        "the lowest-indexed marker wins when planted earlier too"
    );

    // A single higher-indexed marker on its own is still reported.
    let only_high = format!("lead {high} tail");
    assert_eq!(machine_marker_hit(&only_high), Some(high));
}

/// A marker straddling the very end of the content must not be reported, and a
/// marker ending exactly at the end must be. Pins the bounds handling of the
/// prefilter scan, whose candidate compare starts at a first-byte hit and so
/// must tolerate a remainder shorter than the needle. Verified discriminating:
/// against an unchecked `&rest[..needle.len()]` compare this panics with
/// `range end index out of range` instead of returning `None`.
#[test]
fn machine_marker_truncated_at_end_of_content_is_not_a_hit() {
    let marker = MACHINE_MARKERS[0];
    let truncated = &marker[..marker.len() - 1];
    assert_eq!(
        machine_marker_hit(&format!("lead {truncated}")),
        None,
        "a marker cut short by the end of the content is not a hit"
    );
    // The full marker at the exact end IS a hit.
    assert_eq!(
        machine_marker_hit(&format!("lead {marker}")),
        Some(marker),
        "a marker ending exactly at the end of the content is a hit"
    );
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

const TEST_EXCEPTION_PATH: &str = "docs/arch/refactor/rev11/backup/evidence/C1/test-exact.md";
const TEST_PIN_PATH: &str = "docs/arch/refactor/rev11/backup/rulings/test-pin.md";
const TEST_MANIFEST_PATH: &str = "scripts/manifests/portability-machine-marker-evidence-exceptions.tsv";
const TEST_RULING_PATH: &str =
    "docs/arch/refactor/rev11/rulings/ARCHITECT-RULING-TEST-SOURCE-POLICY.md";

fn write_fixture(root: &std::path::Path, rel: &str, bytes: &[u8]) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("fixture path has a parent"))
        .expect("create fixture parent");
    std::fs::write(path, bytes).expect("write fixture");
}

fn manifest_row(path: &str, digest: &str, pin: &str) -> String {
    format!("authority-evidence\t{path}\t{digest}\t{pin}\tC1\tremove after C1 acceptance\n")
}

fn manifest_bytes(rows: &str) -> Vec<u8> {
    format!("class\tpath\tsha256\tpin_document\towner\tretirement_gate\n{rows}").into_bytes()
}

fn valid_rail_fixture() -> (tempfile::TempDir, Vec<String>) {
    let dir = tempfile::tempdir().expect("create fixture root");
    let marker_bytes = format!("authority transcript: {}\n", MACHINE_MARKERS[0]).into_bytes();
    let evidence_digest = sha256_hex(&marker_bytes);
    write_fixture(dir.path(), TEST_EXCEPTION_PATH, &marker_bytes);
    write_fixture(
        dir.path(),
        TEST_PIN_PATH,
        format!("Evidence SHA-256: `{evidence_digest}`\n").as_bytes(),
    );
    let manifest = manifest_bytes(&manifest_row(
        TEST_EXCEPTION_PATH,
        &evidence_digest,
        TEST_PIN_PATH,
    ));
    write_fixture(dir.path(), TEST_MANIFEST_PATH, &manifest);
    write_fixture(
        dir.path(),
        TEST_RULING_PATH,
        format!("Exception manifest SHA-256: `{}`\n", sha256_hex(&manifest)).as_bytes(),
    );
    let tracked = vec![
        TEST_EXCEPTION_PATH.to_string(),
        TEST_PIN_PATH.to_string(),
        TEST_MANIFEST_PATH.to_string(),
        TEST_RULING_PATH.to_string(),
    ];
    (dir, tracked)
}

// @ai-generated - Exercises exact-byte admission and every fail-closed rail required by the source-policy ruling.
#[test]
fn evidence_exception_admits_only_the_exact_registered_bytes() {
    let (dir, tracked) = valid_rail_fixture();
    let exceptions = validate_evidence_exception_manifest(
        dir.path(),
        &tracked,
        TEST_MANIFEST_PATH,
        TEST_RULING_PATH,
        1,
    )
    .expect("valid exact evidence rail");
    assert!(exceptions.contains_key(TEST_EXCEPTION_PATH));

    write_fixture(
        dir.path(),
        TEST_EXCEPTION_PATH,
        format!("changed transcript: {}\n", MACHINE_MARKERS[0]).as_bytes(),
    );
    let errors = validate_evidence_exception_manifest(
        dir.path(),
        &tracked,
        TEST_MANIFEST_PATH,
        TEST_RULING_PATH,
        1,
    )
    .expect_err("altered evidence bytes must fail");
    assert!(errors.iter().any(|error| error.contains("digest mismatch")));
}

// @ai-generated - Proves the manifest is itself bound by the ratified ruling digest.
#[test]
fn evidence_exception_manifest_digest_is_ratified() {
    let (dir, tracked) = valid_rail_fixture();
    let mut changed = std::fs::read(dir.path().join(TEST_MANIFEST_PATH)).expect("read manifest");
    changed.extend_from_slice(b"\n");
    write_fixture(dir.path(), TEST_MANIFEST_PATH, &changed);
    let errors = validate_evidence_exception_manifest(
        dir.path(),
        &tracked,
        TEST_MANIFEST_PATH,
        TEST_RULING_PATH,
        1,
    )
    .expect_err("manifest-only digest change must fail");
    assert!(errors.iter().any(|error| error.contains("manifest digest")));
}

// @ai-generated - Proves an unlisted marker-bearing evidence file is still rejected.
#[test]
fn unlisted_marker_bearing_evidence_is_rejected() {
    let dir = tempfile::tempdir().expect("create fixture root");
    write_fixture(
        dir.path(),
        TEST_EXCEPTION_PATH,
        format!("unlisted {}\n", MACHINE_MARKERS[0]).as_bytes(),
    );
    let report = scan_marker_violations(
        dir.path(),
        &[TEST_EXCEPTION_PATH.to_string()],
        &std::collections::BTreeMap::new(),
    );
    assert!(report.read_errors.is_empty());
    assert_eq!(report.violations.len(), 1);
}

// @ai-generated - Proves exact rows cannot broaden admission beyond the two permitted roots.
#[test]
fn listed_path_outside_permitted_roots_is_rejected() {
    let (dir, mut tracked) = valid_rail_fixture();
    let outside = "docs/contributing/portable.md";
    let bytes = format!("outside {}\n", MACHINE_MARKERS[0]).into_bytes();
    let digest = sha256_hex(&bytes);
    write_fixture(dir.path(), outside, &bytes);
    write_fixture(
        dir.path(),
        TEST_PIN_PATH,
        format!("Evidence SHA-256: `{digest}`\n").as_bytes(),
    );
    let manifest = manifest_bytes(&manifest_row(outside, &digest, TEST_PIN_PATH));
    write_fixture(dir.path(), TEST_MANIFEST_PATH, &manifest);
    write_fixture(
        dir.path(),
        TEST_RULING_PATH,
        format!("Exception manifest SHA-256: `{}`\n", sha256_hex(&manifest)).as_bytes(),
    );
    tracked.push(outside.to_string());
    let errors = validate_evidence_exception_manifest(
        dir.path(),
        &tracked,
        TEST_MANIFEST_PATH,
        TEST_RULING_PATH,
        1,
    )
    .expect_err("outside-root row must fail");
    assert!(errors.iter().any(|error| error.contains("permitted roots")));
}

// @ai-generated - Proves wildcard and duplicate path rows are rejected structurally.
#[test]
fn wildcard_and_duplicate_exception_paths_are_rejected() {
    let (dir, tracked) = valid_rail_fixture();
    let bytes = std::fs::read(dir.path().join(TEST_EXCEPTION_PATH)).expect("read evidence");
    let digest = sha256_hex(&bytes);
    let wildcard = manifest_row(
        "docs/arch/refactor/rev11/evidence/C1/*.md",
        &digest,
        TEST_PIN_PATH,
    );
    let duplicate = format!(
        "{}{}",
        manifest_row(TEST_EXCEPTION_PATH, &digest, TEST_PIN_PATH),
        manifest_row(TEST_EXCEPTION_PATH, &digest, TEST_PIN_PATH)
    );
    for (name, rows, count) in [("wildcard", wildcard, 1usize), ("duplicate", duplicate, 2)] {
        let manifest = manifest_bytes(&rows);
        write_fixture(dir.path(), TEST_MANIFEST_PATH, &manifest);
        write_fixture(
            dir.path(),
            TEST_RULING_PATH,
            format!("Exception manifest SHA-256: `{}`\n", sha256_hex(&manifest)).as_bytes(),
        );
        let errors = validate_evidence_exception_manifest(
            dir.path(),
            &tracked,
            TEST_MANIFEST_PATH,
            TEST_RULING_PATH,
            count,
        )
        .expect_err("malformed path set must fail");
        assert!(
            errors.iter().any(|error| error.contains(name)),
            "expected {name} error, got {errors:?}"
        );
    }
}

// @ai-generated - Proves malformed digests and missing pin documents fail closed.
#[test]
fn malformed_digest_and_missing_pin_document_are_rejected() {
    let (dir, tracked) = valid_rail_fixture();
    let cases = [
        (
            "malformed digest",
            manifest_row(TEST_EXCEPTION_PATH, "not-a-sha256", TEST_PIN_PATH),
        ),
        (
            "missing pin document",
            manifest_row(
                TEST_EXCEPTION_PATH,
                &"0".repeat(64),
                "docs/arch/refactor/rev11/rulings/missing.md",
            ),
        ),
    ];
    for (expected, rows) in cases {
        let manifest = manifest_bytes(&rows);
        write_fixture(dir.path(), TEST_MANIFEST_PATH, &manifest);
        write_fixture(
            dir.path(),
            TEST_RULING_PATH,
            format!("Exception manifest SHA-256: `{}`\n", sha256_hex(&manifest)).as_bytes(),
        );
        let errors = validate_evidence_exception_manifest(
            dir.path(),
            &tracked,
            TEST_MANIFEST_PATH,
            TEST_RULING_PATH,
            1,
        )
        .expect_err("malformed authority row must fail");
        assert!(
            errors.iter().any(|error| error.contains(expected)),
            "expected {expected} error, got {errors:?}"
        );
    }
}

// @ai-generated - Proves both directions of exception liveness: no stale row and no marker without a row.
#[test]
fn evidence_exception_rows_are_live_in_both_directions() {
    let (dir, tracked) = valid_rail_fixture();
    let clean_bytes = b"portable transcript\n";
    let clean_digest = sha256_hex(clean_bytes);
    write_fixture(dir.path(), TEST_EXCEPTION_PATH, clean_bytes);
    write_fixture(
        dir.path(),
        TEST_PIN_PATH,
        format!("Evidence SHA-256: `{clean_digest}`\n").as_bytes(),
    );
    let stale_manifest = manifest_bytes(&manifest_row(
        TEST_EXCEPTION_PATH,
        &clean_digest,
        TEST_PIN_PATH,
    ));
    write_fixture(dir.path(), TEST_MANIFEST_PATH, &stale_manifest);
    write_fixture(
        dir.path(),
        TEST_RULING_PATH,
        format!(
            "Exception manifest SHA-256: `{}`\n",
            sha256_hex(&stale_manifest)
        )
        .as_bytes(),
    );
    let errors = validate_evidence_exception_manifest(
        dir.path(),
        &tracked,
        TEST_MANIFEST_PATH,
        TEST_RULING_PATH,
        1,
    )
    .expect_err("row retained after marker deletion must fail");
    assert!(errors
        .iter()
        .any(|error| error.contains("stale exception row")));

    write_fixture(
        dir.path(),
        TEST_EXCEPTION_PATH,
        format!("restored {}\n", MACHINE_MARKERS[0]).as_bytes(),
    );
    let report = scan_marker_violations(
        dir.path(),
        &[TEST_EXCEPTION_PATH.to_string()],
        &std::collections::BTreeMap::new(),
    );
    assert_eq!(
        report.violations.len(),
        1,
        "row removal cannot hide a marker"
    );
}
