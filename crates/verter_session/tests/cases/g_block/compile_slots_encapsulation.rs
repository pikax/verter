//! Architecture guard: every read / write / invalidation of the
//! per-profile compile-output slots routes through the typed
//! compile-output node, NOT through direct
//! `ProfileState::compile_slots` field access.
//!
//! The `compile_slots` field stays `pub(crate)` because the typed
//! node module (`crate::cache_runtime::compile_output_node`) and the
//! struct module (`crate::types`) are sibling top-level modules of
//! `verter_session`, so there is no language mechanism that makes the
//! field reachable from the node but unreachable from
//! `host_upsert` / `host_lifecycle` / `analysis_io`. Encapsulation is
//! therefore enforced by the gateway-helper pattern plus this static
//! needle-scan guard.
//!
//! Forbidden in production source OUTSIDE the two boundary files:
//!  - `.compile_slots` (field access via dot — `.get` / `.insert` /
//!    `.remove` / `.clear` / `.is_empty` / `.contains_key`, etc.)
//!  - `compile_slot_for_node(`
//!  - `compile_slot_insert_for_node(`
//!  - `compile_slot_remove_for_node(`
//!  - `compile_slots_clear_for_node(`
//!
//! The two boundary files that ARE allowed to name these symbols:
//!  - `crates/verter_session/src/types.rs` — the four gateway helpers
//!    plus the field declaration (the encapsulation boundary).
//!  - `crates/verter_session/src/cache_runtime/compile_output_node.rs`
//!    — the typed node, the only caller of the gateway helpers.
//!
//! Discipline mirrors `no_carrier_verdict_db.rs`:
//!  - scans ONLY `crates/*/src/**/*.rs` (production source),
//!  - skips `_tests.rs` / `tests.rs` / files under a `tests/` segment,
//!  - strips line, block, and `#[cfg(test)] mod` modules before
//!    matching, so doc comments and inline tests do not trip the gate.
//!
//! The audit-tag string literal `"compile_slots"` (e.g.
//! `push_cache_drained_at_upsert("compile_slots", ...)`) is NOT a
//! field access — it has no leading dot — so the `.compile_slots`
//! needle does not flag it. The `invalidate_compile_slots(` /
//! `invalidate_compile_slots` method name is likewise not flagged: the
//! character before `compile_slots` there is `_`, not `.`.

use std::path::{Path, PathBuf};

/// The two production files permitted to name the compile-slot field /
/// gateway helpers — the encapsulation boundary itself.
const BOUNDARY_FILE_NAMES: &[&str] = &["types.rs", "compile_output_node.rs"];

/// The forbidden gateway-helper call tokens (each ends in `(` so it
/// matches a call, not a doc reference to the bare name).
const FORBIDDEN_CALL_TOKENS: &[&str] = &[
    "compile_slot_for_node(",
    "compile_slot_insert_for_node(",
    "compile_slot_remove_for_node(",
    "compile_slots_clear_for_node(",
];

/// The forbidden field-access token. Matched only when the character
/// immediately AFTER `compile_slots` is not an identifier char, so a
/// `.compile_slots_clear_for_node(` access is NOT counted here (it is
/// caught by [`FORBIDDEN_CALL_TOKENS`]) and the bare field decl
/// (`compile_slots:` — no leading dot) is never matched.
const FORBIDDEN_FIELD_TOKEN: &str = ".compile_slots";

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

/// True for files whose contents are test-only (siblings named
/// `*_tests.rs` or `tests.rs`, or anything inside a `tests/` segment).
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

/// True when `path`'s file name is one of the encapsulation-boundary
/// files that ARE permitted to name the compile-slot symbols.
fn is_boundary_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    BOUNDARY_FILE_NAMES.contains(&name)
}

/// Walk a `crates/*/src/` tree and collect every `.rs` file that is
/// production source (NOT a test file).
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
        {
            out.push(path);
        }
    }
}

/// Collect every production `.rs` file under `crates/*/src/`.
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

/// Replace `//` line comments and `/* ... */` block comments with
/// equivalent whitespace, preserving newlines so line numbers stay
/// stable. Skips comment-like sequences inside regular and raw string
/// literals (the literal bytes are preserved, so a `"compile_slots"`
/// string is NOT stripped — but it also lacks a leading dot, so the
/// field-token matcher does not flag it).
fn strip_comments(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let n = bytes.len();
    let mut i = 0usize;
    while i < n {
        let c = bytes[i];
        // Raw string: r"..."  /  r#"..."#  /  r##"..."##  ...
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
            // Not a raw string — fall through to normal handling.
        }
        // Regular string literal "..." with \" escape handling.
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
        // Line comment //
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            let mut k = i;
            while k < n && bytes[k] != b'\n' {
                out.push(b' ');
                k += 1;
            }
            i = k;
            continue;
        }
        // Block comment /* ... */ with nesting support.
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

/// Replace the body of every `#[cfg(test)] mod NAME { ... }` block
/// with whitespace (newlines preserved). Inline test modules live in
/// production source files but are test-only.
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

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// True iff `line` contains the `.compile_slots` field access at a
/// trailing identifier boundary. The trailing-boundary requirement
/// excludes `.compile_slots_clear_for_node` (next char `_`), which is
/// a gateway-helper call counted separately.
fn line_has_field_access(line: &str) -> bool {
    let bytes = line.as_bytes();
    let needle = FORBIDDEN_FIELD_TOKEN.as_bytes();
    let n = needle.len();
    if bytes.len() < n {
        return false;
    }
    let mut i = 0usize;
    while i + n <= bytes.len() {
        if &bytes[i..i + n] == needle {
            let after = i + n;
            let after_ok = after == bytes.len() || !is_ident_char(bytes[after]);
            if after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// True iff `line` contains any forbidden gateway-helper call token.
/// Each token ends in `(`, so it inherently matches a call expression
/// at a right boundary; the left boundary is checked so a longer
/// identifier ending in the token (e.g. a hypothetical
/// `xcompile_slot_for_node(`) is not flagged.
fn line_has_gateway_call(line: &str) -> bool {
    let bytes = line.as_bytes();
    for token in FORBIDDEN_CALL_TOKENS {
        let needle = token.as_bytes();
        let n = needle.len();
        if bytes.len() < n {
            continue;
        }
        let mut i = 0usize;
        while i + n <= bytes.len() {
            if &bytes[i..i + n] == needle {
                let before_ok = i == 0 || !is_ident_char(bytes[i - 1]);
                if before_ok {
                    return true;
                }
            }
            i += 1;
        }
    }
    false
}

fn line_is_violation(line: &str) -> bool {
    line_has_field_access(line) || line_has_gateway_call(line)
}

/// Main gate: no production source file OTHER than the two boundary
/// files may directly access the compile-slot field or call a gateway
/// helper.
#[test]
fn compile_slots_access_is_encapsulated_behind_typed_node() {
    let files = collect_production_sources();

    let mut violations: Vec<(PathBuf, Vec<usize>)> = Vec::new();
    for file in &files {
        if is_boundary_file(file) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let processed = preprocess(&text);
        let lines: Vec<usize> = processed
            .lines()
            .enumerate()
            .filter_map(|(i, l)| {
                if line_is_violation(l) {
                    Some(i + 1)
                } else {
                    None
                }
            })
            .collect();
        if !lines.is_empty() {
            violations.push((file.clone(), lines));
        }
    }

    assert!(
        violations.is_empty(),
        "Architecture rule violation: production source accesses the \
         per-profile compile-output slots directly (`.compile_slots` field \
         access or a `compile_slot*_for_node(` gateway-helper call) outside \
         the two encapsulation-boundary files (`types.rs`, \
         `cache_runtime/compile_output_node.rs`). All compile-output cache \
         state (reads, writes, AND invalidation) MUST route through the \
         typed `CompileOutputNodeFactValidatedSession` methods \
         (`lookup` / `publish` / `remove` / `peek_*` / \
         `clear_compile_outputs_for_file` / `peek_template_analysis`). \
         Direct field access creates a second mutation path the typed \
         admission boundary cannot police.\n\nViolations:\n{violations:#?}"
    );
}

/// Self-test: the scanner catches a deliberate violation AND does not
/// false-positive on the legitimate non-field-access shapes (the
/// audit-tag string literal, the `invalidate_compile_slots` method
/// name, the bare field declaration, the gateway-helper call inside a
/// longer identifier).
#[test]
fn compile_slots_guard_self_test() {
    // Forbidden field-access shapes — must be flagged.
    assert!(
        line_has_field_access("    cc.compile_slots.get(&profile_hash);"),
        "self-test: scanner missed `.compile_slots.get(...)` field access"
    );
    assert!(
        line_has_field_access("entry.compile_slots.clear();"),
        "self-test: scanner missed `.compile_slots.clear()` field access"
    );
    assert!(
        line_has_field_access("profile.compile_slots.remove(&h)"),
        "self-test: scanner missed `.compile_slots.remove(...)` field access"
    );

    // Forbidden gateway-helper calls — must be flagged.
    for call in [
        "self.compile_slot_for_node(profile_hash)",
        "profile_state.compile_slot_insert_for_node(h, slot);",
        "profile_state.compile_slot_remove_for_node(h);",
        "profile_state.compile_slots_clear_for_node();",
    ] {
        assert!(
            line_has_gateway_call(call),
            "self-test: scanner missed forbidden gateway call: `{call}`"
        );
    }

    // Legitimate non-violation shapes — must NOT be flagged.
    let legit = [
        // Audit-tag string literal — no leading dot.
        "push_cache_drained_at_upsert(\"compile_slots\", &canonical_id);",
        // Method NAME containing the substring, but preceded by `_`.
        "host.invalidate_compile_slots(\"/src/App.vue\");",
        "pub fn invalidate_compile_slots(&self, id: &str) {",
        // Bare field declaration — no leading dot.
        "    pub(crate) compile_slots: FxHashMap<u64, CompileSlot>,",
        // Field access that is actually the clear gateway (caught by
        // the gateway-call matcher, NOT the field matcher — so the
        // field matcher must reject it to avoid double-attribution).
        // The `.compile_slots_clear_for_node(` form: trailing char is
        // `_`, so the field matcher's boundary check rejects it.
    ];
    for line in legit {
        assert!(
            !line_has_field_access(line),
            "self-test: field-access scanner false-positives on: `{line}`"
        );
    }

    // The `.compile_slots_clear_for_node(` form must NOT be flagged by
    // the FIELD matcher (trailing `_` fails the boundary), but IS
    // flagged by the GATEWAY-call matcher.
    let clear_call = "session_state.compile_slots_clear_for_node();";
    assert!(
        !line_has_field_access(clear_call),
        "self-test: field matcher must not flag the clear-gateway call \
         shape (it is a gateway call, not a `.compile_slots` field access)"
    );
    assert!(
        line_has_gateway_call(clear_call),
        "self-test: gateway-call matcher must flag the clear-gateway call"
    );

    // The method name `invalidate_compile_slots` must NOT be flagged
    // by the gateway-call matcher either (it has no `_for_node(` form).
    assert!(
        !line_has_gateway_call("host.invalidate_compile_slots(\"/x.vue\");"),
        "self-test: gateway-call matcher false-positives on the \
         `invalidate_compile_slots` method name"
    );
}
