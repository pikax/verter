//! LEGACY_GATE_SELF — `single_language_classifier` + `ffi_no_silent_vue_default`
//! static architecture guards.
//!
//! `single_language_classifier` pins the file-language routing authority:
//!
//!  * exactly ONE language-kind definition exists in the workspace — the
//!    open `FileLanguage` descriptor in `crates/verter_language` (no
//!    `FileKind` re-introduction; the retired enum names also sit in the
//!    `no_legacy_walker.rs::RETIRED_SYMBOLS` gate);
//!  * carrier path-extension literals (`.vue` / `.svelte` / `.astro`)
//!    appear ONLY in `verter_language` (the registry's own rows) plus a
//!    NAMED frozen allowlist of Vue-owned paths (Vue compiler/codegen
//!    paths, the Vue adapter surfaces, and the not-yet-swept LSP/MCP
//!    feature files). The allowlist is shrink-only: removing literals
//!    from a file means deleting its entry; ADDING a file to the list is
//!    an architecture decision, not a convenience.
//!
//! `ffi_no_silent_vue_default` pins the FFI classification semantics: no
//! silent `"vue"` default exists in `verter_ffi` — an absent kind
//! classifies through `LanguageRegistry::classify_static(path)` and a
//! missing path is a typed error (behavior halves live in
//! `verter_ffi::convert::tests`).
//!
//! Scanner discipline mirrors `no_legacy_walker.rs`: production
//! `crates/*/src/**/*.rs` only, comments + inline `#[cfg(test)]` modules
//! stripped, `LEGACY_GATE_SELF`-marked files skipped.

use std::path::{Path, PathBuf};

/// Files whose first lines carry `LEGACY_GATE_SELF` are scanner code.
const SELF_MARKER: &str = "LEGACY_GATE_SELF";

/// The ONE crate allowed to define the language descriptor and to name
/// carrier extensions as data rows.
const LANGUAGE_AUTHORITY_SEGMENT: &str = "crates/verter_language/src";

/// Frozen Vue-owned allowlist for carrier path-extension literals,
/// workspace-relative. Shrink-only.
const CARRIER_LITERAL_ALLOWLIST: &[&str] = &[
    // Vue compiler / parser paths (frozen-Vue surface; component-name
    // suffix strips). The `ide/script/{setup,options_api}.rs` rows are GONE:
    // dropping the in-project `.vue`-import specifier rewrite removed their last
    // carrier-extension literal (this shrink-only list contracts as each surface
    // sheds its carrier literal).
    "crates/verter_compiler/src/compile/helpers.rs",
    "crates/verter_compiler/src/ide/mod.rs",
    // Import-resolution extension tables (resolution data, not
    // classification — `extension_priority` / project-coverage /
    // eval-extension lists stay carrier-aware by design).
    "crates/verter_workspace/src/filesystem.rs",
    "crates/verter_workspace/src/memory.rs",
    "crates/verter_workspace/src/project_graph.rs",
    "crates/verter_napi/src/lib.rs",
    "crates/verter_wasm/src/lib.rs",
    // Vue-semantic session surfaces (Vue parse/extract paths; rows
    // shrink as each surface moves behind the Vue carrier accessors).
    "crates/verter_session/src/host_manage.rs",
    "crates/verter_session/src/host_manage/prepared_decl.rs",
    "crates/verter_session/src/host_resolve/virtual_file_pipeline.rs",
    "crates/verter_session/src/resolver_core/component_meta/direct_macro.rs",
    // Workspace resolver `.vue`-aware routing (resolution data, not
    // classification).
    "crates/verter_workspace/src/resolver.rs",
    // LSP / MCP / tooling feature files (frozen here so no NEW file grows
    // a literal). The LSP feature / server routing files and the MCP tool
    // surface were de-Vue-gated (carrier routing is carrier-generic, pinned
    // by `carrier_lsp_routing_has_no_hardcoded_vue_gate` and
    // `mcp_routing_has_no_hardcoded_vue_gate`), so their rows are gone from
    // this shrink-only list.
    "crates/verter_lsp/src/test_harness.rs",
    "crates/verter_tsc/src/tsconfig.rs",
    // The DX-baseline measurement harness — a deliberately self-contained
    // reference reimplementation used for perf comparison, NOT the shared
    // production classifier. Its `authored_uri_for` (`.vue.tsx`/`.vue.ts` →
    // `.vue`) and the import-rewrite specifier handling do `.vue`-aware path
    // stripping inside the baseline only; they never feed the production
    // `LanguageRegistry`. Frozen here so no NEW production literal grows.
    "crates/verter_dx_baseline/src/artifact_overlay.rs",
    "crates/verter_dx_baseline/src/materialize.rs",
    // The Svelte CSS-conformance corpus generator — a dev/CI-only,
    // non-published tooling crate. Its lone `.svelte` literal COUNTS the
    // fixtures the generator EMITS (the generator's fixed output format); it
    // performs no production language routing and never feeds the production
    // `LanguageRegistry` (the same self-contained-tooling distinction as
    // `verter_dx_baseline`). Frozen here so no NEW production literal grows.
    "crates/verter_svelte_conformance/src/generate.rs",
];

#[test]
fn single_language_classifier() {
    let files = collect_production_sources();
    let root = workspace_root();

    let mut file_kind_hits: Vec<String> = Vec::new();
    let mut file_language_defs: Vec<String> = Vec::new();
    let mut literal_violations: Vec<String> = Vec::new();

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
            // (a) No second language-kind enum: the retired `FileKind` /
            // `ExportGraphFileKind` identifiers must not reappear.
            if line_contains_identifier(line, "FileKind")
                || line_contains_identifier(line, "ExportGraphFileKind")
            {
                file_kind_hits.push(format!("{rel}:{}", idx + 1));
            }
            // (a) The open descriptor is DEFINED once, in verter_language.
            if line.contains("enum FileLanguage") {
                file_language_defs.push(rel.clone());
            }
            // (b) Carrier extension-CHECK literals confined to the
            // authority + the frozen allowlist. Two pattern families
            // (fixture paths like "/src/App.vue" are NOT extension
            // checks and are deliberately out of scope here):
            //  - `ends_with(".vue")`-style suffix classification;
            //  - the bare extension string literal (`".vue"` /
            //    `".svelte"` / `".astro"`) — suffix strips, extension
            //    tables, equality matches.
            if rel.starts_with(LANGUAGE_AUTHORITY_SEGMENT) {
                continue;
            }
            if CARRIER_LITERAL_ALLOWLIST.contains(&rel.as_str()) {
                continue;
            }
            if let Some(ext) = line_has_carrier_extension_check(line) {
                literal_violations.push(format!("{rel}:{} `{ext}`", idx + 1));
            }
        }
    }

    assert!(
        file_kind_hits.is_empty(),
        "retired language-kind enums reintroduced in production source — \
         `verter_language::FileLanguage` is the single language-kind \
         definition.\nHits:\n{file_kind_hits:#?}"
    );
    assert_eq!(
        file_language_defs,
        vec!["crates/verter_language/src/language.rs".to_string()],
        "`FileLanguage` must be defined exactly once, in verter_language"
    );
    assert!(
        literal_violations.is_empty(),
        "carrier path-extension literals outside verter_language + the \
         frozen Vue allowlist. Route through \
         `LanguageRegistry::classify_static` / `carrier_extensions()` \
         instead of matching extensions by hand.\nHits:\n{literal_violations:#?}"
    );
}

/// The frozen allowlist is shrink-only AND must stay accurate: every
/// listed file still exists and still contains at least one carrier
/// literal (stale rows must be deleted, keeping the list honest).
#[test]
fn carrier_literal_allowlist_is_live() {
    let root = workspace_root();
    let mut stale: Vec<String> = Vec::new();
    for rel in CARRIER_LITERAL_ALLOWLIST {
        let path = root.join(rel);
        let Ok(text) = std::fs::read_to_string(&path) else {
            stale.push(format!("{rel} (missing file)"));
            continue;
        };
        let processed = preprocess(&text);
        let has_literal = processed
            .lines()
            .any(|line| line_has_carrier_extension_check(line).is_some());
        if !has_literal {
            stale.push(format!("{rel} (no carrier literal left — delete row)"));
        }
    }
    assert!(
        stale.is_empty(),
        "CARRIER_LITERAL_ALLOWLIST has stale rows (shrink-only list):\n{stale:#?}"
    );
}

#[test]
fn ffi_no_silent_vue_default() {
    let root = workspace_root();
    let ffi_src = root.join("crates/verter_ffi/src");
    let mut files = Vec::new();
    collect_production_rs(&ffi_src, &mut files);
    assert!(
        !files.is_empty(),
        "verter_ffi production sources must be scannable"
    );

    let mut violations: Vec<String> = Vec::new();
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
            if line.contains("unwrap_or(\"vue\")") || line.contains("unwrap_or_else(|| \"vue\"") {
                violations.push(format!("{rel}:{} silent vue default", idx + 1));
            }
            if line_contains_identifier(line, "ffi_file_kind_to_host") {
                violations.push(format!("{rel}:{} retired ffi_file_kind_to_host", idx + 1));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "verter_ffi must not default an absent file kind to \"vue\" — an \
         absent kind classifies via `LanguageRegistry::classify_static` \
         (path) or returns a typed error (no path). Gated rows REQUIRE an \
         explicit kind string at the FFI boundary.\nHits:\n{violations:#?}"
    );
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

fn line_contains_identifier(line: &str, ident: &str) -> bool {
    let bytes = line.as_bytes();
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

/// `Some(ext)` when the (comment-stripped) line performs a carrier
/// extension CHECK: an `ends_with(".vue")`-style suffix classification
/// or a bare extension string literal (`".vue"` / `".svelte"` /
/// `".astro"` — suffix strips, extension tables, equality matches).
/// Fixture paths like `"/src/App.vue"` are NOT extension checks and do
/// not match.
fn line_has_carrier_extension_check(line: &str) -> Option<&'static str> {
    for ext in [".vue", ".svelte", ".astro"] {
        let bare_literal = format!("\"{ext}\"");
        if line.contains(&bare_literal) {
            return Some(ext);
        }
    }
    None
}

// ===== discriminating self-tests for the extension-check detector =====

#[test]
fn extension_check_detector_discriminates() {
    assert_eq!(
        line_has_carrier_extension_check(r#"if path.ends_with(".vue") {"#),
        Some(".vue")
    );
    assert_eq!(
        line_has_carrier_extension_check(r#"let stripped = name.trim_end_matches(".vue");"#),
        Some(".vue")
    );
    assert_eq!(
        line_has_carrier_extension_check(r#"if ext == ".svelte" {"#),
        Some(".svelte")
    );
    // Fixture paths and registry constructors are NOT extension checks.
    assert_eq!(
        line_has_carrier_extension_check(r#"let id = "/src/App.vue";"#),
        None
    );
    assert_eq!(
        line_has_carrier_extension_check("let lang = FileLanguage::vue();"),
        None
    );
    assert_eq!(
        line_has_carrier_extension_check(r#"let glob = "**/*.svelte".to_string();"#),
        None
    );
}
