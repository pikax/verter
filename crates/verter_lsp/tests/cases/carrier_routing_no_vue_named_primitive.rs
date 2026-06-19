//! Static architecture guard: NO new Vue-named GENERIC identifiers in the
//! carrier-routing / provider-sync / position-mapping modules.
//!
//! This is the NAMING half of the carrier-routing guard pair (the literal half
//! is `carrier_routing_no_vue_gate.rs`, which bans executable `.vue`/`"vue"`
//! GATES). This guard bans a SUBTLER regression: a CARRIER-GENERIC routing /
//! provider-sync / watcher / position-mapper primitive (re)introduced with a
//! Vue-flavoured NAME (`vue_resync_ids`, `sync_imported_vue_files`,
//! `vue_position_to_tsx_offset`, `prepare_non_vue_provider_sync`, …). Those
//! names operate on EVERY framework carrier (`.vue`, `.svelte`, …) or on the
//! plain-script non-carrier surface, so a Vue name silently advertises a
//! Vue-only primitive and keeps attracting Vue-only behaviour — stranding
//! every OTHER carrier at less than full parity. This is the "whack-a-mole"
//! class: each review pass kept finding one more `vue_`-named generic
//! primitive. The guard ends it by failing on ANY `vue`/`Vue`-substring
//! identifier in production code in the scanned modules outside a NARROW
//! allowlist of the genuinely Vue-INTRINSIC names.
//!
//! Scope: the production routing / provider-sync / position-mapping source.
//! Test code (`#[cfg(test)]` blocks + `*_tests.rs` files), comments, and
//! string literals are stripped before scanning (a `vue`/`Vue` inside a
//! comment, a test name, or the `"vue"` npm specifier string is NOT an
//! identifier the runtime routes on).
//!
//! The allowlist is the genuinely Vue-INTRINSIC identifier set: Vue-SFC
//! template-syntax attribute mappers (`@event` / `:prop` → `vue_attr` family),
//! the Vue hover-label rewriter, and the Vue runtime-API classification (which
//! lives in `verter_semantic`, not here, but is referenced by name). Every
//! OTHER `vue`/`Vue` identifier in these modules is a carrier-generic or
//! non-carrier primitive that MUST carry a carrier-neutral (`carrier_*` /
//! `non_carrier_*`) name.
//!
//! Registered in `CRITICAL_RULE_GUARDS` under "Framework Adapter Substrate"
//! as `carrier_routing_has_no_vue_named_generic_primitive`. Documented in the
//! `/framework-adapters` skill.

use std::fs;
use std::path::{Path, PathBuf};

/// Production routing / provider-sync / position-mapping source roots scanned
/// by the guard (relative to the crate `src/`).
const SCAN_DIRS: &[&str] = &["features", "server", "documents", "tsgo"];
const SCAN_FILES: &[&str] = &[
    "server_utils.rs",
    "background_drain.rs",
    "provider_sync.rs",
    "sync_coordinator.rs",
    "workspace_scanner.rs",
];

/// Files excluded from the NAMING scan because they are Vue-SPECIFIC FEATURE
/// codegen (not carrier routing). `extract_component.rs` is the "extract
/// selection to a new Vue SFC" code action — intrinsically Vue, de-Vue-gating
/// it is a separate framework-feature concern.
const SCAN_FILE_EXCLUSIONS: &[&str] = &["extract_component.rs"];

/// The genuinely Vue-INTRINSIC identifier allowlist. These are NOT
/// carrier-generic — they map Vue-SFC template syntax (`@event` / `:prop`),
/// rewrite Vue hover labels, or name the Vue runtime-API classification. They
/// are allowed to keep their `vue`/`Vue` name; everything else must be
/// carrier-neutral.
///
/// Match is WHOLE-IDENTIFIER (exact), so a new `vue_*` generic primitive can
/// never sneak in by being a prefix/suffix of an allowlisted name.
const VUE_INTRINSIC_ALLOWLIST: &[&str] = &[
    // Vue-SFC template-syntax attribute mappers (`@click` / `:prop`).
    "jsx_prop_to_vue_attr",
    "extract_vue_attr_label",
    "replace_primary_label_with_vue_attr",
    "vue_attr",
    "vue_label",
    "vue_event_attr_label",
    "emit_vue_attr",
    // The Vue hover kind label (a Vue-SFC concept on the hover surface).
    "vue_kind_label",
    // Vue style-block input (the `<style>` SFC section) + style helpers.
    "VueStyleInput",
    // Vue runtime-API classification (lives in `verter_semantic`; referenced
    // here by type/field name only) + the hover surface for a Vue runtime-API
    // binding (`ref()`, `computed()`, the `vue_api` field on a binding).
    "VueApiClassification",
    "VueApiCallSite",
    "VueApiCallAnalysis",
    "vue_api",
    "vue_api_calls",
    "has_vue_api_call",
    "vue_api_hover_at_offset",
    // The Vue built-in component tags (`<Transition>`, `<KeepAlive>`, … — Vue's
    // SFC-intrinsic builtin element set, completed in the template).
    "VUE_BUILTINS",
    // The `FileLanguage::is_vue()` predicate (a registry-backed Vue language
    // check that lives in `verter_session`; referenced by name in carrier
    // docs/tests).
    "is_vue",
];

fn crate_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

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
        // `*_tests.rs` / `tests.rs` are test fixtures, not routing.
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

/// Strip comments AND string/char literal CONTENTS from a code line, returning
/// only the executable identifier-bearing text. A `vue`/`Vue` inside a comment
/// (`// rewrites .vue imports`) or a string literal (`"vue"` npm specifier,
/// `"vue_to_tsx validation failed"` log) is NOT a routed identifier, so it must
/// not flag. String/char literal bodies are blanked (replaced with spaces) so
/// their length is preserved but no token survives.
///
/// MULTI-LINE-string aware: handles both regular `"…"` and RAW (`r#"…"#`)
/// string literals that span several source lines. A multi-line fixture (e.g.
/// a `r#"<template>…{{ msg }}…"#"` SFC source in a test) must NOT leak its `{` /
/// `}` into the brace-depth counter — otherwise the surrounding `#[cfg(test)]`
/// scope would be torn early and a fixture token wrongly flagged.
#[derive(Clone, Copy, PartialEq)]
enum StrState {
    /// Not inside a string.
    None,
    /// Inside a regular `"…"` / `'…'` string opened with the given quote byte.
    Regular(u8),
    /// Inside a raw `r#…"…"#…` string with the given number of `#` hashes.
    Raw(usize),
}

fn strip_comments_and_strings(code: &str, state: StrState) -> (String, StrState) {
    let bytes = code.as_bytes();
    let mut out = String::with_capacity(code.len());
    let mut i = 0;
    let mut st = state;
    while i < bytes.len() {
        let b = bytes[i];
        match st {
            StrState::Regular(q) => {
                if b == b'\\' && i + 1 < bytes.len() {
                    out.push(' ');
                    out.push(' ');
                    i += 2;
                    continue;
                }
                if b == q {
                    st = StrState::None;
                }
                out.push(' ');
                i += 1;
                continue;
            }
            StrState::Raw(hashes) => {
                // A raw string closes on `"` followed by exactly `hashes` `#`.
                if b == b'"' {
                    let mut h = 0;
                    while i + 1 + h < bytes.len() && bytes[i + 1 + h] == b'#' {
                        h += 1;
                    }
                    if h >= hashes {
                        st = StrState::None;
                        for _ in 0..(1 + hashes) {
                            out.push(' ');
                        }
                        i += 1 + hashes;
                        continue;
                    }
                }
                out.push(' ');
                i += 1;
                continue;
            }
            StrState::None => {}
        }
        // Raw-string opener: `r"…"` / `r#"…"#` / `br"…"` etc.
        if (b == b'r' || b == b'b') && {
            // optional leading `b` then `r`
            let mut j = i;
            if bytes[j] == b'b' {
                j += 1;
            }
            j < bytes.len() && bytes[j] == b'r'
        } {
            let mut j = i;
            if bytes[j] == b'b' {
                j += 1;
            }
            // bytes[j] == b'r'
            let mut k = j + 1;
            let mut hashes = 0;
            while k < bytes.len() && bytes[k] == b'#' {
                hashes += 1;
                k += 1;
            }
            if k < bytes.len() && bytes[k] == b'"' {
                // Open a raw string with `hashes` hashes; blank the opener.
                st = StrState::Raw(hashes);
                for _ in i..=k {
                    out.push(' ');
                }
                i = k + 1;
                continue;
            }
            // Not a raw-string opener — fall through (treat `r`/`b` as ident).
        }
        if b == b'"' || b == b'\'' {
            st = StrState::Regular(b);
            out.push(' ');
            i += 1;
            continue;
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            break; // line comment
        }
        if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
            if let Some(end) = code[i + 2..].find("*/") {
                for _ in 0..(end + 4) {
                    out.push(' ');
                }
                i = i + 2 + end + 2;
                continue;
            }
            break; // opens here, closes later — handled by caller block-comment state
        }
        out.push(b as char);
        i += 1;
    }
    (out, st)
}

/// Whether the line leaves a block comment open (string-aware so a `"/*"` in a
/// literal does not suppress following lines).
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
            break;
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

/// One flagged violation: file, 1-based line, the offending identifier.
type Violation = (String, usize, String);

/// Tokenize a code line into Rust identifiers and return every identifier whose
/// (case-insensitive) lowercased form CONTAINS `"vue"` and is NOT on the
/// whole-identifier allowlist.
fn vue_named_identifiers(code: &str) -> Vec<String> {
    let mut found = Vec::new();
    let bytes = code.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let is_ident_start = b == b'_' || b.is_ascii_alphabetic();
        if !is_ident_start {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() {
            let c = bytes[i];
            if c == b'_' || c.is_ascii_alphanumeric() {
                i += 1;
            } else {
                break;
            }
        }
        let ident = &code[start..i];
        if ident.to_ascii_lowercase().contains("vue") && !VUE_INTRINSIC_ALLOWLIST.contains(&ident) {
            found.push(ident.to_string());
        }
    }
    found
}

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
    let mut pending_cfg_test = false;
    let mut cfg_test_depth: Option<i32> = None;
    let mut depth: i32 = 0;
    let mut str_state = StrState::None;

    for (idx, raw) in src.lines().enumerate() {
        let lineno = idx + 1;
        let mut line = raw.to_string();

        // A multi-line string in progress consumes the whole line (no
        // executable tokens, no brace counting) until it closes.
        if str_state != StrState::None {
            let (_, next) = strip_comments_and_strings(&line, str_state);
            str_state = next;
            continue;
        }

        if in_block_comment {
            if let Some(end) = line.find("*/") {
                line = line[end + 2..].to_string();
                in_block_comment = false;
            } else {
                continue;
            }
        }

        let (code, next_state) = strip_comments_and_strings(&line, StrState::None);
        str_state = next_state;
        if let Some(open) = code_opens_block_comment(&line) {
            in_block_comment = open;
        }

        let trimmed = code.trim();

        if trimmed.contains("#[cfg(test)]") {
            pending_cfg_test = true;
        }

        let opens = code.matches('{').count() as i32;
        let closes = code.matches('}').count() as i32;

        if pending_cfg_test {
            if opens > 0 {
                // A `#[cfg(test)]` immediately before a braced item (`mod tests
                // { … }` / a test `fn … { … }`): skip its whole body.
                cfg_test_depth = Some(depth);
                pending_cfg_test = false;
            } else if !trimmed.is_empty() && opens == 0 && code.contains(';') {
                // A `#[cfg(test)]` on a NON-braced item (`#[cfg(test)] use …;`)
                // applies to that single statement only — it does NOT arm a
                // wholesale brace-skip (otherwise the next production `{` would
                // be wrongly swallowed and the real `mod tests` later missed).
                pending_cfg_test = false;
            }
        }

        let inside_cfg_test = cfg_test_depth.is_some();

        if !inside_cfg_test && !trimmed.is_empty() {
            for ident in vue_named_identifiers(&code) {
                violations.push((rel.clone(), lineno, ident));
            }
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

#[test]
fn carrier_routing_has_no_vue_named_generic_primitive() {
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
    violations.dedup();
    assert!(
        violations.is_empty(),
        "Carrier-routing NAMING guard violations: production routing / provider-sync /\n\
         position-mapping code carries a Vue-named GENERIC identifier. A carrier-generic\n\
         primitive (routes EVERY carrier — `.vue`, `.svelte`, …) must be named `carrier_*`;\n\
         a plain-script non-carrier primitive must be named `non_carrier_*` / `plain_script_*`.\n\
         Only genuinely Vue-INTRINSIC names (Vue-SFC `@event`/`:prop` attr mappers, the Vue\n\
         hover label, the Vue runtime-API classification) are allowlisted.\n\
         Rename the identifier carrier-neutral, or — if it is genuinely Vue-intrinsic — add it\n\
         to `VUE_INTRINSIC_ALLOWLIST` with a justifying comment.\n\n\
         Violations (file:line: identifier):\n  {}",
        violations
            .iter()
            .map(|(rel, ln, id)| format!("{rel}:{ln}: {id}"))
            .collect::<Vec<_>>()
            .join("\n  "),
    );
}

// ───────────────────────── discrimination self-tests ─────────────────────

/// Per-line detector mirroring the scanner's decision for one already-handled
/// (comment/string-stripped happens inside) code line.
fn flags(line: &str) -> Vec<String> {
    let (code, _) = strip_comments_and_strings(line, StrState::None);
    vue_named_identifiers(&code)
}

#[test]
fn flags_generic_vue_named_primitives() {
    // The exact pre-change generic primitives the whack-a-mole kept finding —
    // each MUST flag.
    assert!(
        !flags("    let mut vue_resync_ids = Vec::new();").is_empty(),
        "`vue_resync_ids` generic watcher queue must flag"
    );
    assert!(!flags("    let vue_delete_ids: Vec<(String, String)> = Vec::new();").is_empty());
    assert!(!flags("    pub(super) sync_imported_vue_files: bool,").is_empty());
    assert!(!flags("    let prewarm_imported_vue_apis = true;").is_empty());
    assert!(!flags("    let imported_vue_priority_ids = collect();").is_empty());
    assert!(!flags("pub fn vue_position_to_tsx_offset_validated() {}").is_empty());
    assert!(!flags("    let tsx = mapper.vue_to_tsx(pos);").is_empty());
    assert!(!flags("    let src = mapper.tsx_to_vue(pos);").is_empty());
    assert!(!flags("fn tsx_range_to_vue_range() {}").is_empty());
    assert!(!flags("    pub vue_line_index: LineIndex,").is_empty());
    assert!(!flags("pub fn prepare_non_vue_provider_sync() {}").is_empty());
    assert!(!flags("pub struct PreparedNonVueProviderSync {}").is_empty());
    assert!(!flags("    let mut vue_classified = classify();").is_empty());
}

#[test]
fn does_not_flag_carrier_neutral_post_change_names() {
    // The carrier-neutral post-change names MUST NOT flag.
    assert!(flags("    let mut carrier_resync_ids = Vec::new();").is_empty());
    assert!(flags("    pub(super) sync_imported_carrier_apis: bool,").is_empty());
    assert!(flags("pub fn carrier_position_to_tsx_offset_validated() {}").is_empty());
    assert!(flags("    let tsx = mapper.carrier_to_tsx(pos);").is_empty());
    assert!(flags("    let src = mapper.tsx_to_carrier(pos);").is_empty());
    assert!(flags("    pub carrier_line_index: LineIndex,").is_empty());
    assert!(flags("pub fn prepare_non_carrier_provider_sync() {}").is_empty());
    assert!(flags("pub struct PreparedNonCarrierProviderSync {}").is_empty());
    assert!(flags("    if carrier_language_for(id).is_some() {").is_empty());
}

#[test]
fn does_not_flag_vue_intrinsic_allowlist() {
    // The genuinely Vue-intrinsic names stay green (allowlisted).
    assert!(flags("    let label = jsx_prop_to_vue_attr(prop);").is_empty());
    assert!(flags("    let a = extract_vue_attr_label(item);").is_empty());
    assert!(flags("    replace_primary_label_with_vue_attr(&mut h, a);").is_empty());
    assert!(flags("    target.vue_attr = Some(attr);").is_empty());
    assert!(flags("    let vue_label = compute();").is_empty());
    assert!(flags("    let vue_kind_label = full.vue_kind_label.clone();").is_empty());
    assert!(flags("    for call in analysis.vue_api_calls.iter() {").is_empty());
    assert!(
        flags("        verter_semantic::analysis::VueApiClassification::OnMounted,").is_empty()
    );
    assert!(flags("    assert!(!svelte.is_vue());").is_empty());
}

#[test]
fn does_not_flag_comments_or_strings() {
    // A `vue`/`Vue` inside a comment is not an identifier.
    assert!(flags("// rewrites `.vue` imports to `.vue.ts` for the vue carrier").is_empty());
    assert!(flags("/// Resolve the FileLanguage for a non-Vue source file").is_empty());
    // A `vue`/`Vue` inside a STRING literal (npm specifier, log message) is not
    // a routed identifier.
    assert!(
        flags("    source: \"vue\".into(),").is_empty(),
        "the `\"vue\"` npm specifier string must NOT flag"
    );
    assert!(flags("    callee_import_source: Some(\"vue\".to_string()),").is_empty());
    assert!(
        flags("    tracing::debug!(\"resynced vue {canonical_id}\");").is_empty(),
        "a `vue` word inside a log string must NOT flag"
    );
    assert!(
        flags("        \"hover: vue_to_tsx validation failed for {}\",").is_empty(),
        "a `vue_to_tsx` word inside a log string must NOT flag"
    );
    // A code line with a trailing comment: executable prefix clean, comment
    // mentions `vue` — must NOT flag.
    assert!(flags("    let x = carrier_language_for(p); // was vue_line_index").is_empty());
}

#[test]
fn multiline_raw_string_braces_do_not_tear_scope() {
    // A multi-line raw string (a test SFC fixture) must be fully blanked across
    // lines so its `{` / `}` never reach the brace-depth counter and its
    // `FileLanguage::vue()` line is not exposed. Drive the stateful stripper
    // directly across the fixture lines.
    let mut st = StrState::None;
    let lines = [
        "        source: Arc::<str>::from(",
        "            r#\"<script setup lang=\"ts\">",
        "defineProps<{ msg: string }>()",
        "</script>",
        "<template><div>{{ msg }}</div></template>\"#,",
        "        ),",
    ];
    let mut all = String::new();
    for l in lines {
        let (code, next) = strip_comments_and_strings(l, st);
        st = next;
        all.push_str(&code);
        all.push('\n');
    }
    // The raw-string body (incl. its braces) is blanked: no stray braces, no
    // `script`/`template`/`div` identifiers survive.
    assert!(
        !all.contains("script"),
        "raw-string body must be blanked: {all:?}"
    );
    assert!(!all.contains("template"));
    assert!(
        st == StrState::None,
        "raw string must be closed by the terminating line"
    );
    // The brace count over the blanked output is balanced (the opener `(` line
    // and the `)` line are the only structural tokens).
    let opens = all.matches('{').count();
    let closes = all.matches('}').count();
    assert_eq!(
        opens, 0,
        "no `{{` should survive from the raw fixture: {all:?}"
    );
    assert_eq!(
        closes, 0,
        "no `}}` should survive from the raw fixture: {all:?}"
    );
}

#[test]
fn detects_a_freshly_introduced_vue_primitive_anywhere_in_the_identifier() {
    // The whack-a-mole class: ANY casing / position of `vue` in a generic
    // identifier flags (the substring test, not just a `vue_` prefix).
    assert!(!flags("    let resync_vue_ids = Vec::new();").is_empty());
    assert!(!flags("fn sync_VueFile_to_provider() {}").is_empty());
    assert!(!flags("    let mapped_to_vue = thing();").is_empty());
    assert!(!flags("pub struct VueRoutingState {}").is_empty());
}
