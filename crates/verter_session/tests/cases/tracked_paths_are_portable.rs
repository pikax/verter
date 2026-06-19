//! Architecture guard: every git-TRACKED path must check out on macOS,
//! Windows, and Linux.
//!
//! Verter is built and tested on all three platforms. A single tracked
//! path that NTFS rejects (an ASCII `:` in a component, a reserved device
//! basename, a trailing dot/space) makes `git checkout` fail on
//! Git-for-Windows for the whole repository, and a case-insensitive
//! collision silently clobbers files on default-case-insensitive APFS and
//! NTFS volumes. This guard enumerates the tracked tree via
//! `git ls-files -z` (raw NUL-separated bytes — the non-`-z` form
//! octal-quotes non-ASCII paths and would corrupt the scan) and enforces
//! five portability rules:
//!
//! 1. Every tracked path is valid UTF-8. APFS mandates valid-UTF-8
//!    filenames, so a path with a stray non-UTF-8 byte (raw 0xFF, a
//!    truncated multi-byte sequence) cannot materialize on macOS at all.
//!    Bytes ≥ 0x80 INSIDE a valid UTF-8 sequence are fine.
//! 2. No component contains an NTFS-illegal byte: `< > : " | ? * \` or a
//!    control byte (0x00–0x1F). All illegal bytes are ASCII, so multi-byte
//!    UTF-8 sequences (every byte ≥ 0x80) can never false-positive — the
//!    tracked non-ASCII Greek `.phase-markers/...{α,β,γ}...` names are
//!    NTFS-legal and must pass.
//! 3. No component ends with `.` or a space (Windows strips both at
//!    create time, so checkout round-trips diverge).
//! 4. No component's basename is a reserved Windows device name
//!    (CON/PRN/AUX/NUL/COM1–COM9/LPT1–LPT9/CONIN$/CONOUT$),
//!    case-insensitive, INCLUDING with any extension (`nul.txt` is just
//!    as illegal as `nul`).
//! 5. No two tracked paths collide case-insensitively (checkout clobber
//!    on case-insensitive filesystems) — folded with `str::to_lowercase()`,
//!    the full Unicode lowercase mapping, which APPROXIMATES the NTFS
//!    $UpCase / APFS case-fold tables: it covers the realistic collision
//!    class (ASCII plus the common bicameral scripts such as Greek and
//!    Cyrillic) but is not byte-identical to either filesystem's exact
//!    fold table. Every tracked relative path is also ≤ 200 bytes
//!    (headroom under Windows MAX_PATH with `core.longpaths` default-off).
//!
//! Logical identifiers (e.g. the oracle harness's `blake3:<hash>` /
//! `sha256:<hash>` tagged digests) are NOT constrained by this guard —
//! only the on-disk path boundary is. The path-boundary mapping for the
//! oracle env corpus is `oracle_core::identity::env_corpus_dir_name`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

/// Resolve the repository root from the crate's manifest dir, so the
/// guard works from any worktree location.
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

/// Enumerate every tracked path as RAW BYTES. `-z` is mandatory: the
/// newline-separated form octal-quotes and double-quotes any path with
/// non-ASCII bytes, which would both corrupt the byte checks and hide
/// the real component boundaries.
fn tracked_paths() -> Vec<Vec<u8>> {
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
    out.stdout
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| p.to_vec())
        .collect()
}

/// Bytes NTFS forbids in a path component. All ASCII — a UTF-8
/// continuation or lead byte (≥ 0x80) never matches.
fn is_ntfs_illegal_byte(b: u8) -> bool {
    matches!(b, b'<' | b'>' | b':' | b'"' | b'|' | b'?' | b'*' | b'\\') || b < 0x20
}

/// Reserved Windows device names, matched case-insensitively against the
/// component's stem (everything before the FIRST `.`): `NUL`, `nul.txt`,
/// and `Nul.tar.gz` are all reserved. The console devices `CONIN$` /
/// `CONOUT$` are reserved WITH the `$` only — bare `CONIN`/`CONOUT` are
/// ordinary names.
fn is_reserved_device_name(component: &[u8]) -> bool {
    let stem = component.split(|&b| b == b'.').next().unwrap_or(component);
    let upper: Vec<u8> = stem.iter().map(|b| b.to_ascii_uppercase()).collect();
    match upper.as_slice() {
        b"CON" | b"PRN" | b"AUX" | b"NUL" | b"CONIN$" | b"CONOUT$" => true,
        [b'C', b'O', b'M', d] | [b'L', b'P', b'T', d] => (b'1'..=b'9').contains(d),
        _ => false,
    }
}

/// Rule 1: a tracked path must be valid UTF-8 — APFS mandates valid-UTF-8
/// filenames, so a non-UTF-8 tracked path cannot materialize on macOS.
/// Returns the decoded path for the rules that need `&str`.
fn decode_utf8(path: &[u8]) -> Option<&str> {
    std::str::from_utf8(path).ok()
}

/// Case-fold a tracked path for collision detection. `str::to_lowercase()`
/// applies the full Unicode lowercase mapping — an APPROXIMATION of the
/// NTFS $UpCase / APFS case-fold tables that covers the realistic
/// collision class (ASCII plus the common bicameral scripts such as Greek
/// and Cyrillic); it is not byte-identical to either filesystem's exact
/// fold table.
fn case_fold(path: &str) -> String {
    path.to_lowercase()
}

#[test]
fn tracked_paths_are_portable_across_platforms() {
    let paths = tracked_paths();
    assert!(
        paths.len() > 1000,
        "suspiciously few tracked paths ({}) — enumeration is broken",
        paths.len()
    );

    let mut violations: Vec<String> = Vec::new();

    // Rule 5: case-insensitive collision detection across the full set,
    // folded with the Unicode-aware `case_fold` (rule 1 guarantees every
    // non-violating path decodes, so the fold runs on `&str`).
    let mut case_folded: HashMap<String, String> = HashMap::new();

    for path in &paths {
        let display = String::from_utf8_lossy(path);

        for component in path.split(|&b| b == b'/') {
            if let Some(&bad) = component.iter().find(|&&b| is_ntfs_illegal_byte(b)) {
                violations.push(format!(
                    "{display}: component contains NTFS-illegal byte {:#04x} ({})",
                    bad,
                    if bad.is_ascii_graphic() {
                        (bad as char).to_string()
                    } else {
                        "control".to_string()
                    }
                ));
            }
            if component.ends_with(b".") || component.ends_with(b" ") {
                violations.push(format!(
                    "{display}: component ends with a dot or space (Windows strips it)"
                ));
            }
            if is_reserved_device_name(component) {
                violations.push(format!(
                    "{display}: component is a reserved Windows device name"
                ));
            }
        }

        if path.len() > 200 {
            violations.push(format!(
                "{display}: relative path is {} bytes (> 200-byte portability budget)",
                path.len()
            ));
        }

        match decode_utf8(path) {
            None => violations.push(format!(
                "{display}: not valid UTF-8 — APFS mandates valid-UTF-8 \
                 filenames, so this path cannot check out on macOS"
            )),
            Some(utf8) => {
                if let Some(prev) = case_folded.insert(case_fold(utf8), utf8.to_string()) {
                    violations.push(format!(
                        "{display}: collides case-insensitively with {prev}"
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "tracked paths must be portable across macOS/Windows/Linux \
         checkouts; {} violation(s):\n  {}",
        violations.len(),
        violations.join("\n  ")
    );
}

#[test]
fn reserved_device_name_matcher_discriminates() {
    assert!(is_reserved_device_name(b"NUL"));
    assert!(is_reserved_device_name(b"nul"));
    assert!(is_reserved_device_name(b"nul.txt"));
    assert!(is_reserved_device_name(b"Nul.tar.gz"));
    assert!(is_reserved_device_name(b"COM1"));
    assert!(is_reserved_device_name(b"lpt9.log"));
    assert!(is_reserved_device_name(b"CONIN$"));
    assert!(is_reserved_device_name(b"conin$"));
    assert!(is_reserved_device_name(b"CONOUT$.txt"));
    assert!(is_reserved_device_name(b"conout$.tar.gz"));
    // Without the `$` the console device names are NOT reserved (unlike
    // CON itself, which is): `CONIN`/`CONOUT` are ordinary names.
    assert!(!is_reserved_device_name(b"conin"));
    assert!(!is_reserved_device_name(b"CONOUT"));
    // The stem is everything before the FIRST dot, so a trailing
    // character fused into the stem is not the device name.
    assert!(!is_reserved_device_name(b"conout$x"));
    assert!(!is_reserved_device_name(b"conin$extra.txt"));
    assert!(!is_reserved_device_name(b"COM0"));
    assert!(!is_reserved_device_name(b"COM10"));
    assert!(!is_reserved_device_name(b"console"));
    assert!(!is_reserved_device_name(b"nullable.rs"));
    assert!(!is_reserved_device_name(b"aux_data"));
}

#[test]
fn ntfs_illegal_byte_matcher_discriminates() {
    for b in [b'<', b'>', b':', b'"', b'|', b'?', b'*', b'\\', 0x00, 0x1F] {
        assert!(is_ntfs_illegal_byte(b), "{b:#04x} must be illegal");
    }
    // Multi-byte UTF-8 (lead and continuation bytes are all >= 0x80) can
    // never match: the Greek phase-marker names must pass. 0xFF is not an
    // NTFS-illegal BYTE either — a path containing it is rejected by the
    // PATH-level UTF-8 validity rule instead (see
    // `utf8_validity_rule_discriminates`).
    for b in [b'a', b'.', b'-', b'_', b' ', 0x80, 0xCE, 0xB1, 0xFF] {
        assert!(!is_ntfs_illegal_byte(b), "{b:#04x} must be legal");
    }
}

#[test]
fn utf8_validity_rule_discriminates() {
    // Raw 0xFF is never valid UTF-8 anywhere in a path: APFS mandates
    // valid-UTF-8 filenames, so such a path cannot check out on macOS.
    assert!(decode_utf8(b"crates/verter\xFF.rs").is_none());
    // A truncated multi-byte sequence (lone lead byte) is equally invalid.
    assert!(decode_utf8(b"docs/\xCE").is_none());
    // Bytes >= 0x80 INSIDE a valid UTF-8 sequence stay legal: the tracked
    // Greek phase-marker names must pass.
    assert!(decode_utf8(".phase-markers/\u{3B1}\u{3B2}\u{3B3}.md".as_bytes()).is_some());
    assert!(decode_utf8(b"crates/verter_session/src/lib.rs").is_some());
}

#[test]
fn case_fold_is_unicode_aware() {
    // Greek capital Alpha (U+0391) vs small alpha (U+03B1) collide on
    // case-insensitive APFS/NTFS but are DISTINCT under ASCII byte
    // folding — the Unicode fold must catch them.
    assert_eq!(case_fold("\u{391}.rs"), case_fold("\u{3B1}.rs"));
    assert_eq!(case_fold("src/README.md"), case_fold("src/readme.MD"));
    assert_ne!(case_fold("a.rs"), case_fold("b.rs"));
    assert_ne!(case_fold("\u{3B1}.rs"), case_fold("\u{3B2}.rs"));
}
