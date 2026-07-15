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

// ───────────────────────── Generated carrier/companion path portability ─────
//
// The guard above covers TRACKED paths. Generated carrier/companion virtual
// files (`Foo.vue.tsx`, `Foo.svelte.tsx`, `Foo.vue.verter.ts`, the
// `Foo.d.vue.ts` declaration overlay, …) are NEVER tracked, so the
// `git ls-files` scan never sees them — yet they DO materialize on disk
// (`@verter` `--api` FS-overlays, the TSC mirror tree) and a non-portable
// generated name (an NTFS-illegal char, a reserved device basename, a
// case-collision) would break checkout/materialization on Windows/macOS just
// like a tracked one. This block applies the SAME portability rules to the
// names the descriptor naming column ACTUALLY produces, derived through the
// real production producers (never a hand-built literal), so a producer that
// minted a non-portable name (e.g. a `blake3:<hash>` digest leaking into a
// companion basename, a reserved stem) is caught here.

use verter_session::framework::descriptor::{
    built_in_descriptors, svelte_descriptor, vue_descriptor, FrameworkAdapterDescriptor,
};

/// Apply rules 1–4 of the tracked-path portability contract (UTF-8 validity is
/// implicit — these are `&str`) to ONE generated component-relative name, pushing
/// a labelled violation for each failure. The name is a single relative path
/// segment chain (no drive/UNC prefix); `label` identifies the producer + carrier
/// source for the failure message.
fn check_generated_name_portable(name: &str, label: &str, violations: &mut Vec<String>) {
    for component in name.split(['/', '\\']) {
        if component.is_empty() {
            continue;
        }
        let bytes = component.as_bytes();
        if let Some(&bad) = bytes.iter().find(|&&b| is_ntfs_illegal_byte(b)) {
            violations.push(format!(
                "{label}: generated name `{name}` component `{component}` contains \
                 NTFS-illegal byte {bad:#04x}"
            ));
        }
        if component.ends_with('.') || component.ends_with(' ') {
            violations.push(format!(
                "{label}: generated name `{name}` component `{component}` ends with a dot \
                 or space (Windows strips it)"
            ));
        }
        if is_reserved_device_name(bytes) {
            violations.push(format!(
                "{label}: generated name `{name}` component `{component}` is a reserved \
                 Windows device name"
            ));
        }
    }
    if name.len() > 200 {
        violations.push(format!(
            "{label}: generated name `{name}` is {} bytes (> 200-byte portability budget)",
            name.len()
        ));
    }
}

/// Every generated carrier/companion identity a descriptor's naming column can
/// produce for `carrier_source`, derived through the REAL production producers
/// (`ide_carrier_identities`, the `.verter.` API surface suffix, the testing-API
/// suffix, the sidecar suffixes, and `declaration_carrier_identity`) — never a
/// hand-built literal. Each is paired with a producer label.
fn generated_identities_for(
    descriptor: &FrameworkAdapterDescriptor,
    carrier_source: &str,
) -> Vec<(String, &'static str)> {
    let mut out: Vec<(String, &'static str)> = Vec::new();
    let Some(naming) = descriptor.virtual_file_naming.as_ref() else {
        return out;
    };
    // IDE carrier identities (Vue: `.tsx` AND `.jsx`; Svelte: `.tsx`).
    for ide in naming.ide_carrier_identities(carrier_source) {
        out.push((ide, "ide_carrier_identity"));
    }
    // The redirect-reached `.verter.` public-API surface.
    if let Some(api_suffix) = naming.api_surface_suffix() {
        out.push((format!("{carrier_source}{api_suffix}"), "api_surface"));
    }
    // The testing-API surface.
    if let Some(test_suffix) = naming.testing_api_suffix {
        out.push((format!("{carrier_source}{test_suffix}"), "testing_api"));
    }
    // Any sidecar surfaces.
    for sidecar in naming.sidecar_suffixes {
        out.push((format!("{carrier_source}{sidecar}"), "sidecar"));
    }
    // The extension-middle `.d.<ext>.ts` declaration overlay.
    if let Some(decl) = descriptor.declaration_carrier_identity(carrier_source) {
        out.push((decl, "declaration_carrier"));
    }
    out
}

/// The built-in framework descriptors paired with their carrier extension (`.vue`,
/// `.svelte`) — the SAME registry the production framework-adapter substrate builds,
/// enumerated through [`built_in_descriptors`] (never a hand-listed pair). A
/// descriptor with no carrier extension (a carrier-less adapter) is dropped.
fn carrier_descriptors_with_extension() -> Vec<(FrameworkAdapterDescriptor, String)> {
    built_in_descriptors()
        .into_iter()
        .filter_map(|d| d.carrier_extension().map(|ext| (d, ext)))
        .collect()
}

/// Every TRACKED carrier source, decoded to UTF-8 and paired with the INDEX (into
/// `descriptors`) of the descriptor whose carrier extension it matches. A tracked
/// path is a carrier when its basename ends with a descriptor's carrier extension
/// preceded by ≥1 stem char (the same append-to-full rule
/// `verter_workspace::path_is_carrier` applies). This derives the source set from the
/// REAL `git ls-files` enumeration the tracked-path guard already uses — not a
/// representative literal list — so a path-specific generated collision/nonportable
/// name on any real tracked carrier is exercised.
fn tracked_carrier_sources(
    descriptors: &[(FrameworkAdapterDescriptor, String)],
) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    for raw in tracked_paths() {
        let Some(path) = decode_utf8(&raw) else {
            continue; // a non-UTF-8 path is a Rule-1 violation flagged by the base guard
        };
        let basename = path.rsplit('/').next().unwrap_or(path);
        for (idx, (_descriptor, ext)) in descriptors.iter().enumerate() {
            // Append-to-full: the basename must be strictly longer than the extension
            // (a bare `.vue` file with an empty stem is not a carrier source).
            if basename.len() > ext.len() && basename.ends_with(ext.as_str()) {
                out.push((path.to_string(), idx));
                break;
            }
        }
    }
    out
}

#[test]
fn generated_carrier_companion_names_are_portable() {
    // Derive the carrier sources from the REAL tracked tree (the same `git ls-files`
    // enumeration the tracked-path guard uses), routed to each REAL framework
    // descriptor by its OWN carrier extension — never a representative literal list.
    // Every real tracked `.vue`/`.svelte` source's generated companion identities
    // (IDE carriers, the `.verter.` API surface, the testing-API surface, sidecars,
    // and the `.d.<ext>.ts` declaration overlay) are checked against the SAME
    // portability rules the base guard applies to tracked paths.
    let descriptors = carrier_descriptors_with_extension();
    assert!(
        !descriptors.is_empty(),
        "no carrier-bearing framework descriptors — the registry enumeration is broken \
         (a portability check over zero descriptors is vacuous)"
    );

    let sources = tracked_carrier_sources(&descriptors);
    assert!(
        !sources.is_empty(),
        "no tracked carrier sources matched the descriptor carrier extensions — the \
         derivation from `git ls-files` is broken (the generated-name check would be vacuous)"
    );

    let mut violations: Vec<String> = Vec::new();
    // The full generated set, each name remembered with its originating carrier source
    // so a collision message names BOTH colliding sources.
    let mut folded: HashMap<String, (String, String)> = HashMap::new();

    for (carrier_source, descriptor_idx) in &sources {
        let (descriptor, _ext) = &descriptors[*descriptor_idx];
        let identities = generated_identities_for(descriptor, carrier_source);
        assert!(
            !identities.is_empty(),
            "tracked carrier source `{carrier_source}` produced NO generated identities — \
             the producer wiring is broken (a portability check over an empty set is vacuous)"
        );
        for (name, producer) in identities {
            let label = format!("{producer}({carrier_source})");
            check_generated_name_portable(&name, &label, &mut violations);
            // Case-insensitive collision across the FULL generated set: two distinct
            // tracked carrier sources must never mint companion names that fold
            // together (a clobber on case-insensitive NTFS/APFS). The base guard
            // already proves the tracked SOURCES don't case-collide, so a collision
            // here means a PRODUCER folded two distinct sources into one name.
            if let Some((prev_name, prev_source)) =
                folded.insert(case_fold(&name), (name.clone(), carrier_source.clone()))
            {
                if prev_name != name || &prev_source != carrier_source {
                    violations.push(format!(
                        "generated names `{name}` (from {carrier_source}) and `{prev_name}` \
                         (from {prev_source}) collide case-insensitively"
                    ));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "generated carrier/companion names must be portable across macOS/Windows/Linux \
         (they materialize on disk but are never tracked); {} violation(s):\n  {}",
        violations.len(),
        violations.join("\n  ")
    );
}

#[test]
fn generated_name_portability_checker_discriminates() {
    // The checker FIRES on a synthetic non-portable generated name (an NTFS-illegal
    // `:` from a leaked digest tag, a reserved device basename, a trailing dot) and
    // is CLEAN on the real producer output — proving it would CATCH a producer that
    // minted a non-portable name and is non-vacuous.
    let mut v = Vec::new();
    check_generated_name_portable("blake3:abc.vue.tsx", "synthetic", &mut v);
    assert!(
        v.iter().any(|m| m.contains("NTFS-illegal")),
        "a `:` in a generated name must trip the portability checker"
    );

    let mut v = Vec::new();
    check_generated_name_portable("src/nul.vue.tsx", "synthetic", &mut v);
    assert!(
        v.iter().any(|m| m.contains("reserved")),
        "a reserved device basename (`nul`) in a generated name must trip the checker"
    );

    let mut v = Vec::new();
    check_generated_name_portable("Trailing .vue.tsx", "synthetic", &mut v);
    // `Trailing ` (component before `/`? no — this is one component). The space is
    // mid-name, not trailing on a component; assert the dedicated trailing case via
    // a component that genuinely ends with a space.
    check_generated_name_portable("dir /App.vue.tsx", "synthetic", &mut v);
    assert!(
        v.iter().any(|m| m.contains("ends with a dot or space")),
        "a component ending with a space must trip the checker"
    );

    // The REAL Vue producer output for a normal source is CLEAN.
    let mut v = Vec::new();
    let descriptor = vue_descriptor();
    for (name, producer) in generated_identities_for(&descriptor, "App.vue") {
        check_generated_name_portable(&name, producer, &mut v);
    }
    assert!(
        v.is_empty(),
        "the real Vue companion names for `App.vue` must be portable; got: {v:?}"
    );

    // The REAL Svelte producer output for a normal source is CLEAN too (both
    // carrier-bearing descriptors are exercised, not just Vue).
    let mut v = Vec::new();
    let svelte = svelte_descriptor();
    for (name, producer) in generated_identities_for(&svelte, "src/lib/Card.svelte") {
        check_generated_name_portable(&name, producer, &mut v);
    }
    assert!(
        v.is_empty(),
        "the real Svelte companion names for `src/lib/Card.svelte` must be portable; got: {v:?}"
    );

    // The real declaration carrier IS produced (extension-middle), proving the
    // declaration-overlay name is covered, not silently skipped.
    assert_eq!(
        vue_descriptor().declaration_carrier_identity("App.vue"),
        Some("App.d.vue.ts".to_string()),
        "the `.d.vue.ts` declaration overlay name must be produced and covered"
    );
    assert_eq!(
        svelte_descriptor().declaration_carrier_identity("src/lib/Card.svelte"),
        Some("src/lib/Card.d.svelte.ts".to_string()),
        "the `.d.svelte.ts` declaration overlay name (full path preserved) must be covered"
    );
}

/// DISCRIMINATING: the tracked-source DERIVATION routes a carrier path to the right
/// descriptor by its carrier extension and skips a non-carrier path — so the rewritten
/// `generated_carrier_companion_names_are_portable` actually exercises the REAL tracked
/// carrier set (enumeration-derived), not a hidden hardcoded list.
#[test]
fn tracked_carrier_source_derivation_discriminates() {
    let descriptors = carrier_descriptors_with_extension();
    // Both built-in carrier descriptors are present with their extensions.
    let exts: Vec<&str> = descriptors.iter().map(|(_, e)| e.as_str()).collect();
    assert!(
        exts.contains(&".vue") && exts.contains(&".svelte"),
        "the carrier-descriptor enumeration must include `.vue` and `.svelte`; got {exts:?}"
    );

    // The REAL derivation over the tracked tree yields a substantial, non-empty set
    // (the repo tracks hundreds of `.vue`/`.svelte` carrier fixtures) — proving the
    // generated-name guard is non-vacuous from the enumeration, not a literal list.
    let sources = tracked_carrier_sources(&descriptors);
    assert!(
        sources.len() > 50,
        "expected many tracked carrier sources from the `git ls-files` enumeration, got {} — \
         the derivation (or the carrier-extension routing) is broken",
        sources.len()
    );
    // Every derived source ends with the carrier extension of the descriptor it routed
    // to (the routing is by real extension, never a blind index).
    for (source, idx) in &sources {
        let (_descriptor, ext) = &descriptors[*idx];
        assert!(
            source.ends_with(ext.as_str()),
            "tracked source `{source}` routed to descriptor with extension `{ext}` but does \
             not end with it"
        );
    }
    // A non-carrier path (`.ts`) and a bare-extension path (empty stem) are NOT routed.
    let non_carrier: Vec<&str> = sources
        .iter()
        .map(|(s, _)| s.as_str())
        .filter(|s| s.ends_with(".ts") && !s.ends_with(".vue") && !s.ends_with(".svelte"))
        .collect();
    assert!(
        non_carrier.is_empty(),
        "a plain `.ts` path must NOT be derived as a carrier source; got {non_carrier:?}"
    );
}

// ───────────────────────── C14: cross-platform path-identity invariants ─────
//
// The external-TS engine relies on ONE canonical path-identity layer
// (`verter_span::path`, re-exported as `verter_workspace::canonicalize_path` /
// `CanonicalPath`) shared by the VFS, the position mapper, and both provider
// adapters. A carrier the provider opens, the diagnostic file the engine reports,
// and the document the editor sends must all canonicalize to ONE id, or a feature
// lands on the wrong file. These tests pin the cross-platform identity invariants
// the real-provider path depends on: Windows drive-casing, UNC forms, and the
// case-insensitive collision class. Symlink / pnpm realpath is a SEPARATE layer
// (the fs realpath, not this lexical identity) and is asserted as such.

use verter_workspace::{canonicalize_path, CanonicalPath};

#[test]
fn windows_drive_casing_canonicalizes_to_one_identity() {
    // `C:\` and `c:\` are the SAME file on Windows; the canonical identity lowers
    // the drive letter so both map to ONE id (the engine may report `C:` while the
    // configured path uses `c:`). The rest of the path keeps its case (correct on
    // case-sensitive Linux).
    let upper = canonicalize_path(r"C:\Users\Dev\App.vue.tsx");
    let lower = canonicalize_path(r"c:\Users\Dev\App.vue.tsx");
    assert_eq!(upper, lower, "drive-letter case must not split identity");
    assert_eq!(
        upper, "c:/Users/Dev/App.vue.tsx",
        "the drive is lowered, separators normalized, the rest case-preserved"
    );
    // The newtype's Eq/Hash agree (so a map keyed by CanonicalPath collapses them).
    assert_eq!(
        CanonicalPath::new(r"C:\Users\Dev\App.vue.tsx"),
        CanonicalPath::new(r"c:\Users\Dev\App.vue.tsx"),
        "CanonicalPath equality must fold the drive case"
    );
    let mut set = std::collections::HashSet::new();
    set.insert(CanonicalPath::new(r"C:\Users\Dev\App.vue.tsx"));
    assert!(
        set.contains(&CanonicalPath::new(r"c:/Users/Dev/App.vue.tsx")),
        "a CanonicalPath-keyed set must treat the drive-case variants as one key"
    );
}

#[test]
fn unc_forms_canonicalize_to_one_identity() {
    // The Windows extended-length / UNC prefixes all denote the same network path;
    // the canonical identity strips `\\?\` and `\\?\UNC\` so a carrier reached via
    // any form is ONE id.
    let raw_unc = canonicalize_path(r"\\server\share\App.vue.tsx");
    let ext_unc = canonicalize_path(r"\\?\UNC\server\share\App.vue.tsx");
    assert_eq!(
        raw_unc, ext_unc,
        "the `\\\\?\\UNC\\` extended form must canonicalize to the bare UNC path"
    );
    assert_eq!(raw_unc, "//server/share/App.vue.tsx");
    // The extended-length DRIVE prefix `\\?\C:\` reduces to the lowered drive form.
    assert_eq!(
        canonicalize_path(r"\\?\C:\repo\Widget.svelte.tsx"),
        "c:/repo/Widget.svelte.tsx",
        "the `\\\\?\\C:\\` extended-drive form must reduce to the lowered drive id"
    );
}

#[test]
fn case_insensitive_collision_folds_for_collision_detection() {
    // Two carrier companion paths differing ONLY by case collide on
    // case-insensitive NTFS/APFS; the portability `case_fold` folds them together so
    // the collision detector (used by the tracked-path guard and the generated-name
    // guard above) catches it. This is the SAME fold the guard relies on, applied to
    // generated companion names.
    assert_eq!(
        case_fold("c:/project/Component.vue.tsx"),
        case_fold("c:/project/component.vue.tsx"),
        "case-only-distinct companion names must fold to one bucket"
    );
    // A genuinely distinct stem does NOT fold together (no false collision).
    assert_ne!(
        case_fold("c:/project/Alpha.vue.tsx"),
        case_fold("c:/project/Beta.vue.tsx"),
        "distinct stems must not collide"
    );
}

/// Symlink / pnpm realpath is a DIFFERENT layer than the lexical canonical
/// identity: `canonicalize_path` is purely lexical (drive-lower + UNC-strip +
/// separator-normalize), it does NOT resolve symlinks. The fs realpath
/// (`std::fs::canonicalize`, what pnpm's virtual store relies on) resolves the link
/// to its target. This test pins BOTH: the OS realpath collapses a symlink to its
/// target, while Verter's lexical identity preserves the path as written (the two
/// layers are distinct and must not be conflated).
///
/// On Windows, creating a directory symlink requires privilege (Developer Mode or
/// admin); when unavailable the symlink-creation is skipped with a clear reason
/// (the lexical-identity assertions still run unconditionally).
#[test]
fn symlink_realpath_is_a_separate_layer_from_lexical_identity() {
    let tmp = tempfile::tempdir().expect("temp dir");
    let target = tmp.path().join("real_pkg");
    std::fs::create_dir_all(target.join("src")).expect("create target dir");
    std::fs::write(target.join("src/Comp.vue"), "<template></template>").expect("write carrier");

    // Lexical identity (unconditional, all platforms): the symlinked path and the
    // target path canonicalize LEXICALLY to distinct ids — Verter does not collapse
    // symlinks at the identity layer (realpath is the VFS/fs layer's job).
    let link_path_str = tmp
        .path()
        .join("linked_pkg")
        .to_string_lossy()
        .replace('\\', "/");
    let target_path_str = target.to_string_lossy().replace('\\', "/");
    assert_ne!(
        canonicalize_path(&format!("{link_path_str}/src/Comp.vue")),
        canonicalize_path(&format!("{target_path_str}/src/Comp.vue")),
        "the lexical canonical identity must NOT collapse a symlink to its target \
         (realpath is a separate layer)"
    );

    // fs realpath (gated): create the symlink and assert std::fs::canonicalize (the
    // pnpm virtual-store realpath behavior) resolves the LINK to the TARGET.
    let link = tmp.path().join("linked_pkg");
    let symlink_made = make_dir_symlink(&target, &link);
    match symlink_made {
        Ok(()) => {
            let via_link = std::fs::canonicalize(link.join("src/Comp.vue"))
                .expect("realpath the file through the symlink");
            let via_target = std::fs::canonicalize(target.join("src/Comp.vue"))
                .expect("realpath the file through the target");
            assert_eq!(
                via_link, via_target,
                "fs realpath (the pnpm-store layer) must resolve the symlink to its target"
            );
        }
        Err(reason) => {
            // Skip-with-reason: the lexical-identity assertion above already ran; the
            // realpath sub-case needs symlink privilege this environment lacks.
            eprintln!("skipping the fs-realpath sub-case (symlink creation unavailable): {reason}");
        }
    }
}

/// Create a directory symlink `link -> target`, returning `Err(reason)` when the
/// platform/privilege does not allow it (Windows without Developer Mode / admin).
fn make_dir_symlink(target: &std::path::Path, link: &std::path::Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_dir(target, link)
            .map_err(|e| format!("symlink_dir requires privilege on Windows: {e}"))
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).map_err(|e| format!("symlink: {e}"))
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (target, link);
        Err("symlinks unsupported on this platform".to_string())
    }
}
