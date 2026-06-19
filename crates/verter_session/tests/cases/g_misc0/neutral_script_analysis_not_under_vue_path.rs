//! LEGACY_GATE_SELF — `neutral_script_analysis_not_under_vue_path`
//! static architecture guard.
//!
//! Framework-neutral OXC script analysis (the raw pre-lowering surface
//! capture and the local type-surface engine) lives under the neutral
//! path `crates/verter_parser/src/utils/oxc/script/` — NOT under the
//! Vue-named path `crates/verter_parser/src/utils/oxc/vue/`. This guard
//! pins that boundary:
//!
//!  * production code neither defines nor imports the framework-neutral
//!    symbols (the `script/type_surface` + `script/raw_surface`
//!    relocation rows: `AnalyzedExternalTypeSource`, `RawSourceSurface`,
//!    `SymbolSpace`, `build_type_context`, `resolve_type_elements`,
//!    `infer_runtime_type`, `capture_statement_surfaces`, …) under/from
//!    EITHER legacy path form — `utils::oxc::vue::script::*` AND
//!    `utils::oxc::vue::resolve_type::*` — in any spelling (the direct
//!    `verter_parser` path, the `verter_compiler::utils` re-export
//!    spelling, or in-crate `crate::utils::oxc::vue::…` paths);
//!  * the legacy module files no longer exist and the neutral home
//!    does;
//!  * no Vue-path re-export alias surfaces the neutral module back
//!    under `utils::oxc::vue::*`.
//!
//! Scanner discipline mirrors `no_legacy_walker.rs`: production
//! `crates/*/src/**/*.rs` only, comments + inline `#[cfg(test)]`
//! modules stripped, `LEGACY_GATE_SELF`-marked files skipped.

use std::path::{Path, PathBuf};

/// Files whose first lines carry `LEGACY_GATE_SELF` are scanner code.
const SELF_MARKER: &str = "LEGACY_GATE_SELF";

/// The framework-neutral script-analysis symbols owned by
/// `verter_parser::utils::oxc::script` (`raw_surface` + `type_surface` +
/// the `bindings` inventory). Importing ANY of these through a
/// `…::oxc::vue::…` path is a boundary violation.
const NEUTRAL_SYMBOLS: &[&str] = &[
    // raw_surface
    "RawSourceSurface",
    "SymbolSpace",
    "CapturedSurface",
    "capture_declaration_surfaces",
    "capture_statement_surfaces",
    "merge_overload_groups",
    // script/bindings.rs — the neutral import/decl/pattern inventory
    "collect_pattern_binding_spans",
    "collect_import_binding_spans",
    "declaration_binding_span",
    "callee_identifier_name",
    // type_surface — context + entry points
    "build_type_context",
    "TypeResolutionContext",
    "InterfaceResolutionEntry",
    "extract_companion_types",
    "resolve_type_elements",
    "resolve_type_elements_with_ctx",
    "resolve_type_elements_with_ctx_ref",
    // type_surface — element surfaces
    "ResolvedElements",
    "ResolvedProp",
    "ResolvedMemberVisibility",
    "ResolvedNamedCallSignature",
    "ResolvedCallPayloadForm",
    "BlockedType",
    "BlockedTypeSurface",
    // type_surface — external program analysis
    "AnalyzedExternalTypeSource",
    "AnalyzedExternalTypeSourceStats",
    "AnalyzedExternalTypeSymbol",
    "AnalyzedExternalTypeSymbolKind",
    "analyze_external_type_program",
    "analyze_external_type_source",
    "resolve_external_type",
    "resolve_external_type_with_canonical",
    "resolve_external_type_with_companion",
    "resolve_external_type_with_companion_and_canonical",
    "resolve_external_type_in_program_with_analyzed_symbol_companion",
    "resolve_external_type_in_program_with_analyzed_symbol_companion_and_canonical",
    "resolve_external_type_in_context_with_analyzed_symbol_companion",
    "resolve_external_type_in_context_with_analyzed_symbol_companion_and_canonical",
    "ImportedTypeBinding",
    "ExtractedTypeBindings",
    "ExtractedExportSurface",
    "extract_export_surface",
    "extract_imported_type_bindings",
    "hash_resolved_type",
    "collect_required_import_names_for_external_type",
    "imported_member_name_for_required_alias",
    "required_import_alias_names_for_binding",
    // type_surface — runtime-type inference
    "infer_runtime_type",
    "RuntimeType",
    "format_runtime_types",
    // type_surface — diagnostics + budget
    "ResolutionDiagnostic",
    "ResolutionDiagnosticKind",
    "DiagnosticLocation",
    "ResolutionBudgetExceeded",
    "take_last_resolution_budget_exceeded",
];

/// Legacy module-path tokens that are banned WHOLESALE in production
/// source: every neutral submodule spelled through the Vue path.
const LEGACY_MODULE_PATH_TOKENS: &[&str] = &[
    "oxc::vue::script::resolve_type",
    "oxc::vue::resolve_type",
    "oxc::vue::script::type_surface",
    "oxc::vue::type_surface",
    "oxc::vue::script::raw_surface",
    "oxc::vue::raw_surface",
];

/// Legacy files that the rehoming deleted; re-creation is forbidden.
const LEGACY_FILES: &[&str] = &[
    "crates/verter_parser/src/utils/oxc/vue/script/raw_surface.rs",
    "crates/verter_parser/src/utils/oxc/vue/script/raw_surface_tests.rs",
    "crates/verter_parser/src/utils/oxc/vue/script/resolve_type",
    "crates/verter_parser/src/utils/oxc/vue/script/resolve_type_tests.rs",
    "crates/verter_parser/src/utils/oxc/vue/script/resolve_type_typed_form_tests.rs",
];

/// The neutral home that must exist and own the neutral symbols.
const NEUTRAL_HOME_FILES: &[&str] = &[
    "crates/verter_parser/src/utils/oxc/script/raw_surface.rs",
    "crates/verter_parser/src/utils/oxc/script/type_surface/mod.rs",
];

/// The Vue-named directory the neutral symbols must stay out of.
const VUE_DIR: &str = "crates/verter_parser/src/utils/oxc/vue";

#[test]
fn neutral_script_analysis_not_under_vue_path() {
    let root = workspace_root();
    let files = collect_production_sources();

    let mut violations: Vec<String> = Vec::new();

    // (1) Legacy module files must be gone; the neutral home must exist.
    for legacy in LEGACY_FILES {
        if root.join(legacy).exists() {
            violations.push(format!("{legacy} still exists (legacy module path)"));
        }
    }
    for home in NEUTRAL_HOME_FILES {
        if !root.join(home).exists() {
            violations.push(format!(
                "{home} missing — the neutral script-analysis home must own these symbols"
            ));
        }
    }

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

        // (2) Banned legacy module-path tokens, any spelling, anywhere.
        for (idx, line) in processed.lines().enumerate() {
            for token in LEGACY_MODULE_PATH_TOKENS {
                if line.contains(token) {
                    violations.push(format!("{rel}:{} `{token}`", idx + 1));
                }
            }
        }

        // (3) `use` statements that reach a neutral symbol through ANY
        // `…::oxc::vue::…` path (handles multi-line grouped imports).
        for stmt in normalized_use_statements(&processed) {
            if !stmt.contains("oxc::vue::") {
                continue;
            }
            for sym in NEUTRAL_SYMBOLS {
                if contains_identifier(&stmt, sym) {
                    violations.push(format!(
                        "{rel} imports neutral symbol `{sym}` through a Vue path: `{stmt}`"
                    ));
                }
            }
        }

        // (4) Definition-site half: the neutral symbols must not be
        // DEFINED under the Vue-named directory, and no Vue-path
        // re-export alias may surface the neutral module.
        if rel.starts_with(VUE_DIR) {
            for (idx, line) in processed.lines().enumerate() {
                for sym in NEUTRAL_SYMBOLS {
                    for def_kw in ["struct", "enum", "trait", "union", "fn", "type"] {
                        if contains_identifier(line, sym)
                            && line.contains(&format!("{def_kw} {sym}"))
                        {
                            violations.push(format!(
                                "{rel}:{} defines neutral symbol `{sym}` under the Vue path",
                                idx + 1
                            ));
                        }
                    }
                }
            }
            for stmt in normalized_use_statements(&processed) {
                if let Some(reason) = vue_path_reexport_violation(&stmt) {
                    violations.push(format!("{rel} {reason}: `{stmt}`"));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "framework-neutral script analysis must live under \
         `verter_parser::utils::oxc::script` (raw_surface + type_surface), \
         never under `utils::oxc::vue::*`. Import it from the neutral \
         path; keep Vue macro/SFC semantics under `vue/script/`.\n\
         Violations:\n{violations:#?}"
    );
}

/// `Some(reason)` when a normalized `use` statement found under the
/// Vue-named directory re-exports the neutral script-analysis module —
/// in ANY path spelling — or binds one of its symbols.
///
/// Spellings covered:
///  * absolute: `…oxc::script::…`;
///  * `super::`-chained relative: any chain of `super::` segments that
///    lands on `script::` resolves outside the Vue tree (the Vue
///    directory owns no sibling `script` module), i.e. the neutral
///    module — and every longer chain (`super::super::script::`)
///    contains the `super::script::` substring;
///  * module-segment through an in-scope alias: the Vue tree owns no
///    `raw_surface`/`type_surface` modules, so those path segments in a
///    re-export always denote the neutral module;
///  * direct binding of a [`NEUTRAL_SYMBOLS`] identifier, regardless of
///    the path that reaches it.
///
/// Plain (non-`pub`) `use` statements are sanctioned delegation — the
/// thinned Vue classifier imports the neutral inventory to delegate to
/// it — and are not flagged.
fn vue_path_reexport_violation(stmt: &str) -> Option<String> {
    let is_reexport = stmt.starts_with("pub use")
        || stmt.starts_with("pub(crate) use")
        || stmt.starts_with("pub(super) use");
    if !is_reexport {
        return None;
    }
    if stmt.contains("oxc::script::") {
        return Some(
            "re-exports the neutral script module under the Vue path (absolute spelling)"
                .to_string(),
        );
    }
    if stmt.contains("super::script::") {
        return Some(
            "re-exports the neutral script module under the Vue path (`super::` spelling)"
                .to_string(),
        );
    }
    for segment in ["raw_surface", "type_surface"] {
        if contains_identifier(stmt, segment) {
            return Some(format!(
                "re-exports the neutral `{segment}` module under the Vue path"
            ));
        }
    }
    for sym in NEUTRAL_SYMBOLS {
        if contains_identifier(stmt, sym) {
            return Some(format!(
                "re-exports neutral symbol `{sym}` under the Vue path"
            ));
        }
    }
    None
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

/// Extract every `use …;` statement from comment-stripped source,
/// whitespace-normalized to a single line (grouped multi-line imports
/// collapse into one statement so path + symbol stay co-resident).
/// A `pub` / `pub(crate)` / `pub(super)` re-export prefix is preserved.
fn normalized_use_statements(processed: &str) -> Vec<String> {
    let bytes = processed.as_bytes();
    let n = bytes.len();
    let mut out = Vec::new();
    let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut i = 0usize;
    while i + 4 <= n {
        if &bytes[i..i + 4] == b"use " && (i == 0 || !is_ident_char(bytes[i - 1])) {
            let mut k = i + 4;
            // `;` (0x3B) never appears as a UTF-8 continuation byte, so
            // this byte scan lands on a real char boundary.
            while k < n && bytes[k] != b';' {
                k += 1;
            }
            let body: String = processed[i..k]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let stmt = match reexport_prefix(bytes, i) {
                Some(prefix) => format!("{prefix} {body}"),
                None => body,
            };
            out.push(stmt);
            i = k + 1;
            continue;
        }
        i += 1;
    }
    out
}

/// `Some(prefix)` when the `use` at `use_start` is a re-export
/// (`pub use` / `pub(crate) use` / `pub(super) use`). Pure byte
/// arithmetic — never slices into a multi-byte codepoint.
fn reexport_prefix(bytes: &[u8], use_start: usize) -> Option<&'static str> {
    let mut k = use_start;
    while k > 0 && bytes[k - 1].is_ascii_whitespace() {
        k -= 1;
    }
    for prefix in ["pub(crate)", "pub(super)", "pub"] {
        let pb = prefix.as_bytes();
        if k >= pb.len() && &bytes[k - pb.len()..k] == pb {
            return Some(prefix);
        }
    }
    None
}

// ===== discriminating self-tests for the detectors =====

#[test]
fn use_statement_normalizer_discriminates() {
    let src = "pub use crate::utils::oxc::vue::script::{\n    build_type_context,\n    ResolvedElements,\n};\nfn unrelated() {}\n";
    let stmts = normalized_use_statements(src);
    assert_eq!(stmts.len(), 1);
    assert!(stmts[0].starts_with("pub use"));
    assert!(stmts[0].contains("oxc::vue::"));
    assert!(contains_identifier(&stmts[0], "build_type_context"));
    assert!(contains_identifier(&stmts[0], "ResolvedElements"));
    // Identifier boundaries hold: `ResolvedProp` is not in this import.
    assert!(!contains_identifier(&stmts[0], "ResolvedProp"));
    // `build_type_context` does not match a superstring identifier.
    assert!(!contains_identifier(
        "fn rebuild_type_context_inner()",
        "build_type_context"
    ));
}

#[test]
fn vue_reexport_detector_discriminates() {
    // Violations: every spelling of a Vue-path re-export alias for the
    // neutral module — absolute, `super::`-chained relative (the natural
    // in-crate spelling), module-segment through a local alias, and
    // direct symbol binding.
    for stmt in [
        // absolute spelling
        "pub use crate::utils::oxc::script::raw_surface::RawSourceSurface;",
        "pub(crate) use crate::utils::oxc::script::type_surface::ResolvedElements;",
        // relative spelling from `vue/script/*` (super::super::script)
        "pub use super::super::script::raw_surface::RawSourceSurface;",
        "pub use super::super::script::raw_surface::*;",
        "pub use super::super::script::type_surface::{ build_type_context, ResolvedElements };",
        "pub(crate) use super::super::script::bindings::{ collect_import_binding_spans };",
        // relative spelling from `vue/mod.rs` (super::script)
        "pub use super::script::type_surface::*;",
        // module-segment spelling through an in-scope alias
        "pub use neutral::raw_surface::*;",
        "pub use neutral_alias::type_surface::ResolvedElements;",
    ] {
        assert!(
            vue_path_reexport_violation(stmt).is_some(),
            "detector must flag Vue-path re-export alias: `{stmt}`"
        );
    }
    // Legitimate Vue-owned re-exports (the live `vue/` surface) and plain
    // (non-`pub`) delegation imports stay clean.
    for stmt in [
        // `vue/mod.rs` re-exporting its OWN `script` child module
        "pub use script::*;",
        "pub use types::*;",
        "pub use shared::ScriptParseContext;",
        "pub use macros::{ find_macro_type_param };",
        "pub use super::macros::{ MacroTypeParams, VueMacroKind };",
        // plain `use` = sanctioned delegation, not a re-export alias
        "use crate::utils::oxc::script::type_surface::{ build_type_context, ResolvedElements };",
        "use super::super::script::bindings::collect_import_binding_spans;",
    ] {
        assert!(
            vue_path_reexport_violation(stmt).is_none(),
            "detector must not flag legitimate Vue-owned surface: `{stmt}`"
        );
    }
}
