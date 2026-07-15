//! LEGACY_GATE_SELF — `plain_script_dialect_from_file_language`
//! static architecture guard.
//!
//! Plain (non-carrier) script files parse under the dialect their
//! classified [`verter_language::FileLanguage`] row declares. The
//! registry is the SOLE plain-script dialect authority:
//!
//!  * the retired path-sniffing function `non_sfc_source_type` must
//!    not exist anywhere in production source (retired-symbol-gate
//!    style identifier ban, all `crates/*/src/**`);
//!  * the session parse/source-type surface (the files that compute
//!    OXC `SourceType`s for file content) must carry NO
//!    extension-suffix dialect sniffing — no `ends_with(".d.ts")` /
//!    `".tsx"` / `".jsx"` / kin, and no `.vue` routing by suffix:
//!    those files consume the classified `FileLanguage` row;
//!  * the replacement authority (`plain_script_source_type`, the
//!    `FileLanguage`-driven mapping) must exist in
//!    `crates/verter_session/src/parse.rs`.
//!
//! The RUNTIME half of this guard lives in
//! `src/plain_script_dialect_tests.rs` (classified-dialect fixtures
//! over `.tsx`/`.jsx`/`.js`/`.mjs`/`.cjs`/`.d.ts`, the JS module-kind
//! hazard pins, Vue byte-identity spot-checks) and in the exhaustive
//! `ScriptSourceType` → OXC `SourceType` parity matrix unit tests
//! beside `oxc_source_type_from_neutral` in `src/parse.rs`.
//!
//! Scanner discipline mirrors `no_legacy_walker.rs`: production
//! `crates/*/src/**/*.rs` only, comments + inline `#[cfg(test)]`
//! modules stripped, `LEGACY_GATE_SELF`-marked files skipped.

use std::path::{Path, PathBuf};

/// Files whose first lines carry `LEGACY_GATE_SELF` are scanner code.
const SELF_MARKER: &str = "LEGACY_GATE_SELF";

/// The retired path-sniffing dialect function. Banned as an identifier
/// in ALL production source.
const RETIRED_SYMBOLS: &[&str] = &["non_sfc_source_type"];

/// The session parse/source-type computation surface: the files that
/// produce OXC `SourceType` values for file content. These consume the
/// classified `FileLanguage` row and must not re-sniff extensions.
const PARSE_SURFACE_FILES: &[&str] = &[
    "crates/verter_session/src/parse.rs",
    "crates/verter_session/src/host_executor.rs",
    "crates/verter_session/src/host_manage/eval_env.rs",
    "crates/verter_session/src/host_manage/eval_program.rs",
];

/// Extension-suffix dialect probes banned on the parse surface. The
/// list covers every extension the language registry classifies (and
/// the `.vue` carrier suffix) — dialect/carrier routing on these files
/// goes through the classified row, never the path.
const BANNED_SUFFIX_LITERALS: &[&str] = &[
    "\".d.ts\"",
    "\".d.mts\"",
    "\".d.cts\"",
    "\".ts\"",
    "\".tsx\"",
    "\".mts\"",
    "\".cts\"",
    "\".js\"",
    "\".jsx\"",
    "\".mjs\"",
    "\".cjs\"",
    "\".vue\"",
];

/// The replacement authority that must exist: the `FileLanguage`-driven
/// plain-script source-type derivation in `parse.rs`.
const AUTHORITY_FILE: &str = "crates/verter_session/src/parse.rs";
const AUTHORITY_SYMBOL: &str = "plain_script_source_type";

#[test]
fn plain_script_dialect_from_file_language() {
    let root = workspace_root();
    let files = collect_production_sources();

    let mut violations: Vec<String> = Vec::new();

    // (1) Retired-symbol ban: `non_sfc_source_type` must not appear as
    // an identifier in any production source.
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let processed = preprocess(&text);
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        for (idx, line) in processed.lines().enumerate() {
            for sym in RETIRED_SYMBOLS {
                if contains_identifier(line, sym) {
                    violations.push(format!(
                        "{rel}:{} retired path-sniffing symbol `{sym}`",
                        idx + 1
                    ));
                }
            }
        }
    }

    // (2) Suffix-sniff ban on the parse/source-type surface.
    for surface in PARSE_SURFACE_FILES {
        let path = root.join(surface);
        let Ok(text) = std::fs::read_to_string(&path) else {
            violations.push(format!("{surface} missing — parse surface moved?"));
            continue;
        };
        let processed = preprocess(&text);
        for (idx, line) in processed.lines().enumerate() {
            if let Some(lit) = suffix_sniff_violation(line) {
                violations.push(format!(
                    "{surface}:{} extension-suffix dialect sniff on {lit}: `{}`",
                    idx + 1,
                    line.trim()
                ));
            }
        }
    }

    // (3) The replacement authority exists where the plan homes it.
    {
        let path = root.join(AUTHORITY_FILE);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let processed = preprocess(&text);
        let defines_authority = processed
            .lines()
            .any(|line| line.contains(&format!("fn {AUTHORITY_SYMBOL}")));
        if !defines_authority {
            violations.push(format!(
                "{AUTHORITY_FILE} must define `{AUTHORITY_SYMBOL}` — the \
                 FileLanguage-driven plain-script source-type authority"
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "plain-script dialect derives from the classified `FileLanguage` row \
         (the `verter_language` registry); session parse code must not \
         re-sniff path extensions and the retired `non_sfc_source_type` \
         must stay deleted.\nViolations:\n{violations:#?}"
    );
}

/// `Some(banned_literal)` when a (comment-stripped) source line probes
/// a path suffix for a registry-classified extension: a banned suffix
/// literal co-resident with a suffix/containment probe call
/// (`ends_with` / `strip_suffix` / `contains`).
fn suffix_sniff_violation(line: &str) -> Option<&'static str> {
    const PROBES: &[&str] = &["ends_with", "strip_suffix", "contains"];
    if !PROBES.iter().any(|probe| line.contains(probe)) {
        return None;
    }
    BANNED_SUFFIX_LITERALS
        .iter()
        .find(|lit| line.contains(*lit))
        .copied()
}

// ===== discriminating self-tests for the detectors =====

#[test]
fn suffix_sniff_detector_discriminates() {
    // Violations: every probe form over a registry-classified suffix.
    for line in [
        r#"if canonical_id.ends_with(".d.ts") {"#,
        r#"        || canonical_id.ends_with(".d.mts")"#,
        r#"if id.ends_with(".tsx") { SourceType::tsx() } else"#,
        r#"let is_jsx = path.ends_with(".jsx");"#,
        r#"if canonical.ends_with(".vue") {"#,
        r#"let trimmed = canonical.strip_suffix(".mjs");"#,
    ] {
        assert!(
            suffix_sniff_violation(line).is_some(),
            "detector must flag suffix dialect sniff: `{line}`"
        );
    }
    // Clean lines: no probe call, or no banned literal.
    for line in [
        r#"let file_language = self.language_classifier.classify(canonical);"#,
        r#"let st = plain_script_source_type(&file_language);"#,
        r#"if name.ends_with("_tests.rs") {"#,
        r#"let ext = ".ts"; // mention without a probe"#,
    ] {
        assert!(
            suffix_sniff_violation(line).is_none(),
            "detector must not flag clean line: `{line}`"
        );
    }
}

#[test]
fn retired_symbol_detector_discriminates() {
    assert!(contains_identifier(
        "let st = non_sfc_source_type(canonical_id);",
        "non_sfc_source_type"
    ));
    // Identifier boundaries hold: a superstring identifier is not a hit.
    assert!(!contains_identifier(
        "fn parse_non_sfc_source_type_matrix() {}",
        "non_sfc_source_type"
    ));
}

// ===== scanner helpers (no_legacy_walker.rs discipline) =====

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    loop {
        if p.join("CLAUDE.md").exists() && p.join("crates").exists() {
            return p;
        }
        if !p.pop() {
            panic!(
                "unable to locate workspace root from {}",
                env!("CARGO_MANIFEST_DIR")
            );
        }
    }
}

fn is_test_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    if name == "tests.rs" || name.ends_with("_tests.rs") {
        return true;
    }
    path.components()
        .any(|c| c.as_os_str().to_str() == Some("tests"))
}

fn is_self_excluded(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    text.lines().take(5).any(|l| l.contains(SELF_MARKER))
}

fn collect_production_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name == "target" || name == "node_modules" || name == ".git" {
                continue;
            }
            collect_production_rs(&path, out);
        } else if path.is_file()
            && path.extension().and_then(|e| e.to_str()) == Some("rs")
            && !is_test_file(&path)
            && !is_self_excluded(&path)
        {
            out.push(path);
        }
    }
}

fn collect_production_sources() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut files: Vec<PathBuf> = Vec::new();
    let crates_dir = root.join("crates");
    let Ok(crates) = std::fs::read_dir(&crates_dir) else {
        return files;
    };
    for entry in crates.flatten() {
        let crate_dir = entry.path();
        if !crate_dir.is_dir() {
            continue;
        }
        let src = crate_dir.join("src");
        if !src.is_dir() {
            continue;
        }
        collect_production_rs(&src, &mut files);
    }
    files
}

/// Replace `//` and `/* ... */` comments with whitespace (newlines
/// preserved), skipping string literals.
fn strip_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let n = bytes.len();
    let mut i = 0usize;
    while i < n {
        let c = bytes[i];
        if c == b'r' {
            let mut j = i + 1;
            let mut hashes = 0usize;
            while j < n && bytes[j] == b'#' {
                hashes += 1;
                j += 1;
            }
            if j < n && bytes[j] == b'"' {
                out.extend_from_slice(&bytes[i..=j]);
                let close: Vec<u8> = std::iter::once(b'"')
                    .chain(std::iter::repeat_n(b'#', hashes))
                    .collect();
                let mut k = j + 1;
                while k + close.len() <= n {
                    if &bytes[k..k + close.len()] == close.as_slice() {
                        out.extend_from_slice(&bytes[(j + 1)..(k + close.len())]);
                        i = k + close.len();
                        break;
                    }
                    out.push(bytes[k]);
                    k += 1;
                }
                if k + close.len() > n {
                    out.extend_from_slice(&bytes[(j + 1)..n]);
                    i = n;
                }
                continue;
            }
        }
        if c == b'"' {
            out.push(b'"');
            let mut k = i + 1;
            while k < n {
                if bytes[k] == b'\\' && k + 1 < n {
                    out.push(bytes[k]);
                    out.push(bytes[k + 1]);
                    k += 2;
                    continue;
                }
                if bytes[k] == b'"' {
                    out.push(b'"');
                    k += 1;
                    break;
                }
                out.push(bytes[k]);
                k += 1;
            }
            i = k;
            continue;
        }
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            let mut k = i;
            while k < n && bytes[k] != b'\n' {
                out.push(b' ');
                k += 1;
            }
            i = k;
            continue;
        }
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            let mut depth = 1u32;
            out.push(b' ');
            out.push(b' ');
            let mut k = i + 2;
            while k < n && depth > 0 {
                if k + 1 < n && bytes[k] == b'/' && bytes[k + 1] == b'*' {
                    depth += 1;
                    out.push(b' ');
                    out.push(b' ');
                    k += 2;
                    continue;
                }
                if k + 1 < n && bytes[k] == b'*' && bytes[k + 1] == b'/' {
                    depth -= 1;
                    out.push(b' ');
                    out.push(b' ');
                    k += 2;
                    continue;
                }
                if bytes[k] == b'\n' {
                    out.push(b'\n');
                } else {
                    out.push(b' ');
                }
                k += 1;
            }
            i = k;
            continue;
        }
        out.push(c);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Blank the bodies of inline `#[cfg(test)] mod NAME { ... }` blocks.
fn strip_inline_test_modules(src: &str) -> String {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut out = bytes.to_vec();
    let needle = b"#[cfg(test)]";
    let mut i = 0usize;
    while i + needle.len() <= n {
        if &bytes[i..i + needle.len()] == needle {
            let mut j = i + needle.len();
            let limit = (i + 200).min(n);
            while j < limit {
                if j + 4 <= n && &bytes[j..j + 4] == b"mod " {
                    break;
                }
                j += 1;
            }
            if j + 4 <= n && &bytes[j..j + 4] == b"mod " {
                let mut k = j + 4;
                while k < n && bytes[k] != b'{' && bytes[k] != b';' {
                    k += 1;
                }
                if k < n && bytes[k] == b'{' {
                    let mut depth = 1i32;
                    let mut m = k + 1;
                    while m < n && depth > 0 {
                        match bytes[m] {
                            b'{' => depth += 1,
                            b'}' => depth -= 1,
                            _ => {}
                        }
                        m += 1;
                    }
                    if m > k + 1 {
                        for slot in &mut out[(k + 1)..(m - 1)] {
                            if *slot != b'\n' {
                                *slot = b' ';
                            }
                        }
                    }
                    i = m;
                    continue;
                }
            }
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn preprocess(src: &str) -> String {
    strip_inline_test_modules(&strip_comments(src))
}

fn contains_identifier(text: &str, ident: &str) -> bool {
    let bytes = text.as_bytes();
    let needle = ident.as_bytes();
    let n = needle.len();
    if n == 0 || bytes.len() < n {
        return false;
    }
    let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut i = 0usize;
    while i + n <= bytes.len() {
        if &bytes[i..i + n] == needle {
            let before_ok = i == 0 || !is_ident_char(bytes[i - 1]);
            let after_ok = i + n == bytes.len() || !is_ident_char(bytes[i + n]);
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}
