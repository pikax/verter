//! Static architecture guard: framework-carrier LSP routing must be
//! carrier-generic.
//!
//! Verter is one shared, carrier-generic substrate (`is_framework_carrier()`,
//! `carrier_language_for`, `path_is_carrier`, `strip_carrier_extension`,
//! `carrier_ide_provider_path`, `carrier_api_provider_path`). The feature /
//! server ROUTING layer must NOT re-introduce a Vue-only gate: an
//! `.is_vue(` call, an `ends_with(".vue")` / `strip_suffix(".vue")` /
//! `trim_end_matches(".vue")` suffix check, or a hardcoded provider/path
//! literal (`.vue.ts` / `.vue.tsx` / `.vue.jsx`) in executable routing keeps
//! attracting Vue-only behaviour and silently strands every OTHER carrier
//! (`.svelte`) at less than full parity.
//!
//! This guard scans the NON-TEST production routing source under
//! `crates/verter_lsp/src` (feature handlers, server methods + sync
//! orchestration, the carrier provider-sync siblings) and FAILS on any
//! executable `.vue`-literal GATE outside a NARROW, documented allowlist.
//!
//! A "gate" is a CLASSIFICATION / ROUTING / PROVIDER-PATH decision keyed on a
//! `.vue` (or `"vue"`-language-id) literal — exactly the shapes that strand
//! every OTHER carrier (`.svelte`). The detector flags, in executable code:
//!
//!   - `.is_vue(` (the legacy carrier predicate),
//!   - a `.vue`-suffix CLASSIFIER: `ends_with` / `strip_suffix` /
//!     `trim_end_matches` / `starts_with` / `contains` against `".vue"`,
//!   - a `.vue` / `"vue"` EQUALITY or `matches!` gate (`== ".vue"`,
//!     `lang == "vue"`, `matches!(x, "vue")`),
//!   - a bare `"vue"` LANGUAGE-ID classifier: `contains` / `starts_with` /
//!     `ends_with` / `strip_prefix` / `strip_suffix` / `trim_*_matches`
//!     against the bare `"vue"` language-id literal (the bare-`"vue"`-prefix
//!     gate — e.g. `language_id.starts_with("vue")` — that the earlier guard
//!     missed),
//!   - a hardcoded carrier PROVIDER literal (`.vue.ts` / `.vue.tsx` /
//!     `.vue.jsx`), including a `format!("{src}.vue.ts")` reconstruction,
//!   - a bare `.vue` PROVIDER-PATH builder: a `format!` / push / concat that
//!     reconstructs a `.vue`-suffixed value into a routing/provider PATH
//!     binding (an `*ide_path*` / `*api_path*` / `*provider*` / `*_path` /
//!     `*_id` target) — NOT generic `.vue` SFC codegen.
//!
//! The allowlist is needle-NARROW. The SSR filename conventions
//! (`.server.vue` / `.client.vue`) are MASKED out of the line span before
//! scanning, so a line that gates on the bare `.vue` BESIDE an SSR check
//! (`ends_with(".vue") || ends_with(".server.vue")`) STILL flags the bare
//! gate — the allowlist never whitelists the whole line. The remaining
//! exclusions: test code (`#[cfg(test)]` blocks + `*_tests.rs` files,
//! stripped), comments (stripped), explicit Svelte-specific `is_svelte()`
//! (a different carrier), and the `extract_component.rs` Vue-SFC-codegen
//! feature (a Vue-specific code action that CREATES a `.vue` file + import —
//! feature codegen, NOT carrier routing; de-Vue-gating extract-to-SFC is a
//! separate framework-feature concern, not LSP carrier routing).
//!
//! It deliberately does NOT allowlist `.vue` in definition / navigation /
//! component-resolution, workspace symbols, component import / drop, watcher
//! carrier routing, provider-path reverse-mapping, or barrel/provider sync —
//! those are exactly the categories that must be carrier-generic.
//!
//! Registered in `CRITICAL_RULE_GUARDS` under "Framework Adapter Substrate"
//! as `carrier_lsp_routing_has_no_hardcoded_vue_gate`. Documented in the
//! `/framework-adapters` skill.

use std::fs;
use std::path::{Path, PathBuf};

/// The production routing source roots scanned by the guard (relative to the
/// crate `src/`). Feature handlers + server methods + the editor-document
/// INGRESS (`documents/`, where the client `languageId` is classified) + the
/// TypeProvider merge / provider-path REVERSE-MAPPING (`tsgo/`) — every surface
/// that classifies or routes a carrier path/language must be carrier-generic.
const SCAN_DIRS: &[&str] = &["features", "server", "documents", "tsgo"];
const SCAN_FILES: &[&str] = &[
    "server_utils.rs",
    "background_drain.rs",
    "provider_sync.rs",
    "sync_coordinator.rs",
    "workspace_scanner.rs",
];

/// Files excluded from the carrier-ROUTING scan because they are Vue-SPECIFIC
/// FEATURE codegen, not routing/classification. `extract_component.rs` is the
/// "extract selection to a new Vue SFC" code action — it CREATES a `.vue`
/// file and its `import … from './X.vue'`. That is intrinsic Vue-feature
/// codegen; de-Vue-gating it is a separate framework-feature concern outside
/// the LSP carrier-routing guard's scope.
const SCAN_FILE_EXCLUSIONS: &[&str] = &["extract_component.rs"];

fn crate_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Recursively collect `.rs` files under `dir`, skipping `*_tests.rs` and
/// `tests.rs` (test fixtures, not routing).
fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs(&path, out);
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.ends_with(".rs") {
            continue;
        }
        if name.ends_with("_tests.rs") || name == "tests.rs" {
            continue;
        }
        if SCAN_FILE_EXCLUSIONS.contains(&name) {
            continue;
        }
        out.push(path);
    }
}

fn scan_files() -> Vec<PathBuf> {
    let src = crate_src();
    let mut files = Vec::new();
    for dir in SCAN_DIRS {
        collect_rs(&src.join(dir), &mut files);
    }
    for file in SCAN_FILES {
        let p = src.join(file);
        if p.is_file() {
            files.push(p);
        }
    }
    files.sort();
    files
}

/// Strip the comment portion of a source line (line `//` comments and the
/// trailing part of a `/* … */` that opens-and-closes on one line), being
/// careful not to treat `//` inside a string literal as a comment. Returns
/// only the executable code prefix. Block comments spanning multiple lines
/// are handled by the line iterator's `in_block_comment` state.
fn strip_comment(code: &str) -> String {
    let bytes = code.as_bytes();
    let mut out = String::with_capacity(code.len());
    let mut i = 0;
    let mut in_str: Option<u8> = None;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_str {
            out.push(b as char);
            if b == b'\\' && i + 1 < bytes.len() {
                // Escaped char inside a string — copy verbatim.
                out.push(bytes[i + 1] as char);
                i += 2;
                continue;
            }
            if b == q {
                in_str = None;
            }
            i += 1;
            continue;
        }
        if b == b'"' || b == b'\'' {
            in_str = Some(b);
            out.push(b as char);
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            break; // line comment — rest is non-executable
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            // Single-line block comment: skip to the closing `*/` if present.
            if let Some(end) = code[i + 2..].find("*/") {
                i = i + 2 + end + 2;
                continue;
            }
            break; // opens here, closes on a later line — handled by caller state
        }
        out.push(b as char);
        i += 1;
    }
    out
}

/// One flagged violation: file, 1-based line, the executable text.
type Violation = (String, usize, String);

/// Walk one file, tracking `#[cfg(test)]` brace depth and multi-line block
/// comments, and flag every executable line that contains a forbidden needle
/// outside the narrow allowlist.
fn file_violations(path: &Path) -> Vec<Violation> {
    let Ok(src) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let rel = path
        .strip_prefix(PathBuf::from(env!("CARGO_MANIFEST_DIR")))
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");

    let mut violations = Vec::new();
    let mut in_block_comment = false;
    // `#[cfg(test)]` skip state: when we see the attribute, the NEXT braced
    // item (a `mod tests { … }` or a test `fn`) is skipped wholesale.
    let mut pending_cfg_test = false;
    let mut cfg_test_depth: Option<i32> = None;
    let mut depth: i32 = 0;

    for (idx, raw) in src.lines().enumerate() {
        let lineno = idx + 1;
        let mut line = raw.to_string();

        // Resolve any open multi-line block comment first.
        if in_block_comment {
            if let Some(end) = line.find("*/") {
                line = line[end + 2..].to_string();
                in_block_comment = false;
            } else {
                continue; // entire line is inside a block comment
            }
        }

        let code = strip_comment(&line);
        // Detect an unterminated block comment opened on this line.
        if let Some(open) = code_opens_block_comment(&line) {
            in_block_comment = open;
        }

        let trimmed = code.trim();

        // `#[cfg(test)]` attribute → arm the skip for the next braced item.
        if trimmed.contains("#[cfg(test)]") {
            pending_cfg_test = true;
        }

        let opens = code.matches('{').count() as i32;
        let closes = code.matches('}').count() as i32;

        if pending_cfg_test && opens > 0 {
            // The cfg(test) item's body starts here.
            cfg_test_depth = Some(depth);
            pending_cfg_test = false;
        }

        let inside_cfg_test = cfg_test_depth.is_some();

        if !inside_cfg_test && !trimmed.is_empty() && line_gate_kind(&code).is_some() {
            violations.push((rel.clone(), lineno, trimmed.to_string()));
        }

        depth += opens - closes;
        if let Some(base) = cfg_test_depth {
            if depth <= base {
                cfg_test_depth = None;
            }
        }
    }
    violations
}

/// Whether the line leaves a block comment open (a trailing `/*` with no
/// matching `*/` after it on the same line). String- and line-comment-aware:
/// a `/*` inside a string literal or after a `//` line comment does NOT open a
/// block comment (otherwise a routing line with a `"/*"` string could wrongly
/// suppress every following executable line — the trivial-satisfiability hole).
fn code_opens_block_comment(line: &str) -> Option<bool> {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut in_str: Option<u8> = None;
    let mut open = false;
    while i < bytes.len() {
        let b = bytes[i];
        if let Some(q) = in_str {
            if b == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if b == q {
                in_str = None;
            }
            i += 1;
            continue;
        }
        if b == b'"' || b == b'\'' {
            in_str = Some(b);
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            break; // line comment — nothing after it opens a block comment
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            if let Some(close_rel) = line[i + 2..].find("*/") {
                i = i + 2 + close_rel + 2;
                open = false;
                continue;
            }
            open = true;
            break;
        }
        i += 1;
    }
    Some(open)
}

/// Mask the SSR filename-convention spans (`.server.vue` / `.client.vue`) out
/// of a code line, replacing JUST those literal substrings with a neutral
/// marker so the bare `.vue` gate BESIDE them is still visible to the scanner.
///
/// This is the needle-NARROW allowlist: it neutralises only the documented SSR
/// needles, never the whole line. A line like
/// `ends_with(".vue") || ends_with(".server.vue")` keeps its bare
/// `ends_with(".vue")` after masking and is correctly flagged.
fn mask_ssr_needles(code: &str) -> String {
    // Order matters only in that `.server.vue` / `.client.vue` both END in
    // `.vue`; replacing the SSR span wholesale removes the `.vue` tail too, so
    // a bare `.vue` elsewhere on the line survives.
    code.replace(".server.vue", ".server.CARRIER")
        .replace(".client.vue", ".client.CARRIER")
}

/// Whether `code` is a `.vue` PROVIDER/ROUTING-PATH builder: a `format!` /
/// `push_str` / `+` concatenation that reconstructs a `.vue`-suffixed value
/// into a routing/provider PATH binding. This is the context-aware half of the
/// gate detector — it distinguishes a provider-path reconstruction
/// (`let ide_path = format!("{src}.vue");`) from generic Vue SFC codegen
/// (`format!("import X from './X.vue'")`), which is NOT a routing gate.
///
/// The cue is a routing/provider-PATH target on the same statement: an
/// `ide_path` / `api_path` / `shadow_path` / `provider*` / `*_path` / `*_id`
/// binding, OR a `.parse()`-into-URI of the built `.vue` value. A bare `.vue`
/// string-build with NO such cue (SFC template / import codegen) is NOT a gate.
fn is_vue_provider_path_builder(code: &str) -> bool {
    // Must build a `.vue` value via a string constructor on this line.
    let builds_vue = (code.contains("format!") && code.contains(".vue"))
        || (code.contains("push_str") && code.contains(".vue"));
    if !builds_vue {
        return false;
    }
    // Routing/provider-path cue on the same statement.
    code.contains("ide_path")
        || code.contains("api_path")
        || code.contains("shadow_path")
        || code.contains("provider_id")
        || code.contains("provider_path")
        || code.contains("provider_specifier")
        || code.contains("_path =")
        || code.contains("_path:")
        || code.contains("_id =")
        || code.contains("_id:")
}

/// Classify a single (SSR-mask-then-tested) executable code line as a Vue-only
/// routing/classification GATE, returning a human-readable gate kind for the
/// failure diagnostic, or `None` when the line is carrier-generic / not a gate.
///
/// Operates on the SSR-MASKED line so a bare `.vue` gate beside an SSR check is
/// still seen. An explicit `is_svelte()` check is a DIFFERENT carrier needle
/// and is never a gate here.
fn line_gate_kind(code: &str) -> Option<&'static str> {
    let masked = mask_ssr_needles(code);

    // (0) Hardcoded carrier PROVIDER literals — including a
    //     `format!("{src}.vue.ts")` reconstruction. These are unambiguous
    //     provider-artifact paths regardless of surrounding context.
    for lit in [".vue.ts", ".vue.tsx", ".vue.jsx"] {
        if masked.contains(lit) {
            return Some("hardcoded carrier provider literal (.vue.ts/.tsx/.jsx)");
        }
    }

    // (1) The legacy Vue carrier predicate.
    if masked.contains(".is_vue(") {
        return Some(".is_vue() carrier predicate");
    }

    // (2) `.vue`-suffix CLASSIFIERS: ends_with / strip_suffix /
    //     trim_end_matches / starts_with / contains against `".vue"`.
    for m in [
        "ends_with(\".vue\")",
        "strip_suffix(\".vue\")",
        "trim_end_matches(\".vue\")",
        "starts_with(\".vue\")",
        "contains(\".vue\")",
    ] {
        if masked.contains(m) {
            return Some(".vue-suffix classification gate (ends_with/strip_suffix/contains/...)");
        }
    }

    // (3) `.vue` EQUALITY gate: `== ".vue"` / `!= ".vue"`.
    if masked.contains("== \".vue\"") || masked.contains("!= \".vue\"") {
        return Some(".vue equality gate (== / !=)");
    }

    // (4) `"vue"`-as-language-id EQUALITY / `matches!` / classification gate.
    //     The bare `"vue"` literal is the Vue runtime npm specifier in
    //     non-gating positions (`source: "vue".into()`, `import … from "vue"`),
    //     so it is a gate ONLY in an equality / `matches!` / classification
    //     context (`contains` / `starts_with` / `ends_with` / `strip_prefix` /
    //     `strip_suffix` / `trim_*_matches` against the bare `"vue"`
    //     language-id literal).
    if masked.contains("== \"vue\"") || masked.contains("!= \"vue\"") {
        return Some("\"vue\" language-id equality gate (== / !=)");
    }
    if masked.contains("matches!") && masked.contains("\"vue\"") {
        return Some("\"vue\" language-id matches! gate");
    }
    for m in [
        "contains(\"vue\")",
        "starts_with(\"vue\")",
        "ends_with(\"vue\")",
        "strip_prefix(\"vue\")",
        "strip_suffix(\"vue\")",
        "trim_start_matches(\"vue\")",
        "trim_end_matches(\"vue\")",
        "trim_matches(\"vue\")",
    ] {
        if masked.contains(m) {
            return Some("\"vue\" language-id classification gate (contains/starts_with/ends_with/strip/trim)");
        }
    }

    // (5) Context-aware `.vue` PROVIDER-PATH builder (not SFC codegen).
    if is_vue_provider_path_builder(&masked) {
        return Some(
            ".vue provider/routing-path builder (format!/push_str into a path/id binding)",
        );
    }

    None
}

#[test]
fn carrier_lsp_routing_has_no_hardcoded_vue_gate() {
    let files = scan_files();
    assert!(
        !files.is_empty(),
        "guard found no production routing files to scan — the scan roots drifted"
    );
    let mut violations = Vec::new();
    for f in &files {
        violations.extend(file_violations(f));
    }
    violations.sort();
    assert!(
        violations.is_empty(),
        "Carrier LSP-routing guard violations: production feature/server routing code\n\
         re-introduced a Vue-only gate or a hardcoded carrier provider literal.\n\
         Route through the carrier-generic substrate instead:\n\
           - `file_language.is_framework_carrier()` when you hold a `FileLanguage`,\n\
           - `crate::server::carrier_language_for(path).is_some()` for URI/canonical routing,\n\
           - `verter_workspace::path_is_carrier` / `strip_carrier_extension` for path helpers,\n\
           - `verter_workspace::carrier_ide_provider_path` / `carrier_api_provider_path`\n\
             (or the resolver `provider_*_for_source` helpers) for provider paths.\n\
         Allowlisted ONLY: `.server.vue`/`.client.vue` SSR conventions (masked,\n\
         not whole-line whitelisted), test code, comments, explicit\n\
         `is_svelte()` checks, and the `extract_component.rs` Vue-SFC-codegen\n\
         feature.\n\n\
         Violations:\n  {}",
        violations
            .iter()
            .map(|(rel, ln, line)| format!("{rel}:{ln}: {line}"))
            .collect::<Vec<_>>()
            .join("\n  "),
    );
}

// ───────────────────────── discrimination self-tests ─────────────────────
//
// These prove the guard's detector FAILS against the pre-change (`.vue`-gated)
// shape and PASSES against the carrier-generic post-change shape, and that
// the comment/test/string stripping + allowlist behave precisely.

/// The exact pre-change executable shapes from the seven named gap sites (and
/// the adjacent carrier-routing sites). The detector MUST flag each.
const PRE_CHANGE_VIOLATING_LINES: &[(&str, &str)] = &[
    ("if !file_language.is_vue() {", ".is_vue("),
    ("if !kind.is_vue() {", ".is_vue("),
    (
        "let stem = filename.strip_suffix(\".vue\").unwrap_or(filename);",
        "strip_suffix(\".vue\")",
    ),
    (
        "if !dropped_uri.ends_with(\".vue\") {",
        "ends_with(\".vue\")",
    ),
    ("if file_language.is_vue() {", ".is_vue("),
    ("if params.uri.ends_with(\".vue\") {", "ends_with(\".vue\")"),
    ("if language.is_vue() {", ".is_vue("),
    (
        "if canonical_id.ends_with(\".vue\") {",
        "ends_with(\".vue\")",
    ),
    (
        "if resolved_target.ends_with(\".vue\") {",
        "ends_with(\".vue\")",
    ),
    (
        "if terminal_id.ends_with(\".vue\") {",
        "ends_with(\".vue\")",
    ),
    ("if !file.uri.ends_with(\".vue\") {", "ends_with(\".vue\")"),
    (".then(|| format!(\"{canonical_id}.ts\"))", ""), // not a needle by itself
    ("let p = format!(\"{src}.vue.ts\");", ".vue.ts"),
    (
        "MockCall::OpenFile if path == \"/w/App.vue.tsx\"",
        ".vue.tsx",
    ),
];

/// The carrier-generic post-change shapes. The detector MUST NOT flag any.
const POST_CHANGE_CLEAN_LINES: &[&str] = &[
    "if !file_language.is_framework_carrier() {",
    "if !kind.is_framework_carrier() {",
    "let stem = verter_workspace::strip_carrier_extension(filename);",
    "if crate::server::carrier_language_for(dropped_uri).is_none() {",
    "if carrier_language_for(&canonical_id).is_some() {",
    "if is_default_export_component_carrier(&resolved_target) {",
    "if verter_workspace::path_is_carrier(specifier) {",
    "verter_workspace::carrier_ide_provider_path(&canonical_id, is_jsx),",
    "verter_workspace::carrier_api_provider_path(canonical_id)",
    "if file_language.is_svelte() {",
    // The carrier-neutral editor-INGRESS classifier replacing a hardcoded
    // `language_id == \"vue\"` branch: the registry maps the editor id.
    ".carrier_for_editor_language_id(language_id)",
    "normalize_carrier_path_owned(&d.path, &carrier_source_exists)",
];

/// Per-line detector mirroring the file scanner's executable-line decision
/// for a single, already-comment-stripped code line.
fn line_flags(code: &str) -> bool {
    line_gate_kind(code).is_some()
}

#[test]
fn detector_flags_every_pre_change_violation() {
    for (line, expected_needle) in PRE_CHANGE_VIOLATING_LINES {
        if expected_needle.is_empty() {
            // A line with no forbidden needle is intentionally NOT flagged —
            // it documents that the bare `.ts` API formula alone is not a
            // gate (it is guarded by a carrier predicate elsewhere).
            assert!(
                !line_flags(line),
                "line without a forbidden needle must NOT flag: {line}"
            );
            continue;
        }
        let code = strip_comment(line);
        assert!(
            line_flags(&code),
            "detector failed to flag pre-change violation ({expected_needle}): {line}"
        );
    }
}

#[test]
fn detector_passes_every_post_change_clean_line() {
    for line in POST_CHANGE_CLEAN_LINES {
        let code = strip_comment(line);
        assert!(
            !line_flags(&code),
            "detector wrongly flagged carrier-generic line: {line}"
        );
    }
}

#[test]
fn comments_are_not_flagged() {
    // Pure comments mentioning the forbidden literals must be stripped.
    assert!(!line_flags(&strip_comment(
        "// rewrites `.vue` imports to `.vue.ts`"
    )));
    assert!(!line_flags(&strip_comment(
        "/// Mirrors provider_id_for_source() for .vue files producing .vue.ts"
    )));
    // A code line with a trailing comment: the executable prefix is clean,
    // the comment mentions a literal — must NOT flag.
    assert!(!line_flags(&strip_comment(
        "let x = carrier_language_for(p); // was: ends_with(\".vue\")"
    )));
}

#[test]
fn ssr_convention_is_masked_not_whole_line_whitelisted() {
    // A line gating ONLY on the SSR convention is masked clean.
    assert!(!line_flags(&strip_comment(
        "if path.ends_with(\".server.vue\") {"
    )));
    assert!(!line_flags(&strip_comment(
        "if path.ends_with(\".client.vue\") {"
    )));
    // A bare `.vue` gate on the SAME shape is still a violation.
    assert!(line_flags(&strip_comment("if path.ends_with(\".vue\") {")));

    // THE REGRESSION THE OLD WHOLE-LINE ALLOWLIST MISSED: a bare `.vue` gate
    // sitting BESIDE an SSR check on one line must STILL flag. The needle-narrow
    // mask removes only the `.server.vue` span, leaving the bare gate visible.
    assert!(
        line_flags(&strip_comment(
            "if uri.ends_with(\".vue\") || uri.ends_with(\".server.vue\") {"
        )),
        "bare `.vue` gate beside an SSR check must still flag (whole-line whitelist hole)"
    );
    assert!(line_flags(&strip_comment(
        "if uri.ends_with(\".client.vue\") || uri.ends_with(\".vue\") {"
    )));
}

#[test]
fn broadened_gate_forms_flag() {
    // `lang == "vue"` language-id equality gate.
    assert!(
        line_flags(&strip_comment("if lang == \"vue\" {")),
        "`lang == \"vue\"` language-id gate must flag"
    );
    assert!(line_flags(&strip_comment(
        "if file.language_id != \"vue\" {"
    )));
    // `matches!(..., "vue")` language-id gate.
    assert!(
        line_flags(&strip_comment(
            "if matches!(language_id.as_str(), \"vue\") {"
        )),
        "`matches!(x, \"vue\")` language-id gate must flag"
    );
    // `.contains(".vue")` classification gate.
    assert!(
        line_flags(&strip_comment("if canonical_id.contains(\".vue\") {")),
        "`.contains(\".vue\")` classification gate must flag"
    );
    // `.contains("vue")` language-id classification gate.
    assert!(line_flags(&strip_comment("if source.contains(\"vue\") {")));
    // `== ".vue"` / `starts_with` / `trim_end_matches`.
    assert!(line_flags(&strip_comment("if ext == \".vue\" {")));
    assert!(line_flags(&strip_comment(
        "if name.starts_with(\".vue\") {"
    )));
    assert!(line_flags(&strip_comment(
        "let s = id.trim_end_matches(\".vue\");"
    )));
}

#[test]
fn bare_vue_language_id_prefix_gate_flags() {
    // THE P1a GAP: a bare `language_id.starts_with("vue")` LANGUAGE-ID prefix
    // gate (NOT `.vue` and NOT an equality) was previously MISSED — the guard
    // only caught `.starts_with(".vue")` and `== "vue"`. A `"vue"`-prefix
    // classifier strands every OTHER carrier (`.svelte` never starts with
    // `"vue"`), so it must flag.
    assert!(
        line_flags(&strip_comment("if language_id.starts_with(\"vue\") {")),
        "`language_id.starts_with(\"vue\")` bare language-id prefix gate must flag"
    );
    // The full family of bare-`"vue"` language-id classifiers must flag too.
    assert!(line_flags(&strip_comment("if lang.ends_with(\"vue\") {")));
    assert!(line_flags(&strip_comment(
        "if let Some(rest) = id.strip_prefix(\"vue\") {"
    )));
    assert!(line_flags(&strip_comment(
        "if let Some(rest) = id.strip_suffix(\"vue\") {"
    )));
    assert!(line_flags(&strip_comment(
        "let s = lang.trim_start_matches(\"vue\");"
    )));
    assert!(line_flags(&strip_comment(
        "let s = lang.trim_end_matches(\"vue\");"
    )));
    assert!(line_flags(&strip_comment(
        "let s = lang.trim_matches(\"vue\");"
    )));

    // DISCRIMINATION: a legit non-gate use of the bare `"vue"` literal stays
    // green — the npm/runtime specifier in a struct field / import source is
    // NOT a classification call, so it does NOT flag. (Proves the new arm is
    // discriminating, not a blanket bare-`"vue"` ban.)
    assert!(
        !line_flags(&strip_comment("source: \"vue\".into(),")),
        "the bare Vue npm specifier in a non-classifier position must NOT flag"
    );
    assert!(
        !line_flags(&strip_comment("let specifier = format!(\"vue/{name}\");")),
        "building a `vue/...` runtime sub-specifier must NOT flag (not a classifier)"
    );
}

#[test]
fn vue_provider_path_builder_flags_but_sfc_codegen_does_not() {
    // A `format!` reconstructing a `.vue` value INTO a provider/routing path
    // binding IS a gate (the routing-path-builder context).
    assert!(
        line_flags(&strip_comment("let ide_path = format!(\"{src}.vue\");")),
        "`format!(\"{{src}}.vue\")` into an ide_path binding must flag"
    );
    assert!(line_flags(&strip_comment(
        "let api_path = format!(\"{canonical_id}.vue\");"
    )));
    // The hardcoded provider literal flags regardless of context.
    assert!(line_flags(&strip_comment(
        "let p = format!(\"{src}.vue.ts\");"
    )));

    // Generic Vue SFC codegen — creating a NEW `.vue` file / its import — is
    // NOT a routing gate and must NOT flag (this is `extract_component`'s shape;
    // it has NO routing/provider-path target cue). The detector stays precise.
    assert!(
        !line_flags(&strip_comment(
            "let import_line = format!(\"import {name} from './{name}.vue'\\n\");"
        )),
        "Vue SFC import codegen must NOT flag (it is feature codegen, not a gate)"
    );
    assert!(
        !line_flags(&strip_comment(
            "format!(\"{dir}{component_name}.vue\").parse()"
        )),
        "Vue SFC sibling-file codegen must NOT flag"
    );
}

#[test]
fn vue_npm_specifier_literal_is_not_a_gate() {
    // The bare `"vue"` runtime npm specifier in a NON-equality position (struct
    // field assignment / import source) is NOT a language-id gate.
    assert!(
        !line_flags(&strip_comment("source: \"vue\".into(),")),
        "the Vue runtime npm specifier in a struct field must NOT flag"
    );
    assert!(!line_flags(&strip_comment(
        "callee_import_source: Some(\"vue\".to_string()),"
    )));
    // A field/accessor named `vue_api_calls` (no string literal) must NOT flag.
    assert!(!line_flags(&strip_comment(
        "for call in analysis.vue_api_calls.iter() {"
    )));
}

#[test]
fn is_svelte_check_is_not_flagged() {
    // An explicit Svelte-specific check is a different carrier entirely.
    assert!(!line_flags(&strip_comment(
        "if file_language.is_svelte() {"
    )));
}

#[test]
fn block_comment_open_detector_is_string_aware() {
    // A `/*` INSIDE a string literal must NOT open a block comment (otherwise
    // a routing line carrying such a string could suppress every following
    // executable line — the trivial-satisfiability hole). The detector must
    // report `false` (not block-comment-open) for these.
    assert_eq!(
        code_opens_block_comment("let s = \"a /* not a comment\";"),
        Some(false)
    );
    assert_eq!(code_opens_block_comment("let s = \"/*\";"), Some(false));
    // A `/*` after a `//` line comment does NOT open a block comment.
    assert_eq!(
        code_opens_block_comment("let x = 1; // trailing /* not open"),
        Some(false)
    );
    // A genuine unterminated block comment DOES open.
    assert_eq!(
        code_opens_block_comment("let x = 1; /* opens here"),
        Some(true)
    );
    // A same-line balanced block comment does NOT leave one open.
    assert_eq!(
        code_opens_block_comment("let x = 1; /* closed */ let y = 2;"),
        Some(false)
    );
}
