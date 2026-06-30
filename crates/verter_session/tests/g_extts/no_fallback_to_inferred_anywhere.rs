//! Guard: `no_fallback_to_inferred_anywhere`.
//!
//! The external-TypeScript engine is PROJECT-BOUND on every backend: a framework
//! carrier reaches the TS engine as a member of its REAL configured project, never
//! through a config-less inferred / single-file Program fallback.
//!
//!   * tsserver — the carrier is a configured-project member via the
//!     `@verter/typescript-plugin`'s `getExternalFiles` + `extraFileExtensions`, so
//!     the transport injects NO inferred-project compiler options and defines NO
//!     `configure_paths` override (it inherits the trait default no-op).
//!   * tsgo (owned `--api`) — the carrier-companion CONTENT flows through the shared
//!     `--lsp` session, while membership is owned by the `--api` checker's
//!     `open_project`: `resolve_for` calls `update_snapshot_open_project(tsconfig)`,
//!     selects `project_for_config(== tsconfig)`, and REQUIRES the carrier in
//!     `project.root_files`, FAILING CLOSED when it is absent. There is NO
//!     inferred/single-file fallback branch; the config-less owned restart backend
//!     (`struct TsgoBackend` + its `ResilientBackend` impl + the config-less
//!     `pub fn new(` constructor + a `tsgo_resilient::new(` wrap) is ABSENT — only
//!     the project-bound owned backend exists.
//!
//! This STATIC guard is the GLOBAL source-level backstop for the project-bound
//! membership contract. It walks the external-TS PRODUCTION source across
//! `crates/verter_lsp/src/**`, `crates/verter_tsc/src/**`, and
//! `crates/verter_type_runtime/src/**` (BOTH the tsserver and the tsgo backend
//! paths), EXCLUDING `*_tests.rs` siblings and comment lines, and FAILS if any
//! inferred-project CONSTRUCTION / OPEN knob appears anywhere.
//!
//! It deliberately keys on CONSTRUCTION/OPEN tokens, NEVER the bare substrings
//! `inferred`/`fallback` — those appear legitimately 100+ times (`ProjectRank::
//! Inferred` is a workspace precedence-tier name; `ProjectPayload::Fallback` /
//! `FallbackMembership` is the vite-config workspace concept; feature-level
//! fallbacks abound: CSS query fallback, document-symbol fallback range,
//! default-export navigation fallback, macro-prop fallback). A guard keyed on those
//! substrings would be a false-positive minefield and prove nothing.
//!
//! The ONE legitimate `compilerOptionsForInferredProjects` CONFIGURATION call lives
//! inside `ExtensionTypeProvider::configure_paths` at
//! `crates/verter_lsp/src/extension_provider.rs`: it CONFIGURES the compiler options
//! tsserver would apply to any inferred project (the standard volar /
//! ts-language-features init pattern); it does NOT construct/open a carrier into an
//! inferred Program. The allow-list is PINNED to that exact shape by BRACE/BODY SCOPE:
//! the request name is permitted at EXACTLY ONE live occurrence, and ONLY when its byte
//! offset falls inside the matched `{ … }` body of the `configure_paths` method ON the
//! `TypeProvider for ExtensionTypeProvider` impl (located by brace-matching from the
//! method signature's opening brace to its close, over a string/char/comment-blanked
//! view so literal braces never miscount). A SECOND occurrence anywhere, an occurrence
//! in a different function, OR an occurrence placed AFTER the method's closing brace but
//! before the next function (in the bare impl scope) is a violation: a real
//! inferred-open / config injection added anywhere outside the method body is caught,
//! not hidden behind either a file-wide pass or a most-recent-`fn` attribution gap.
//!
//! DISCRIMINATING: the self-test below proves every predicate FIRES on a synthetic
//! reintroduction (the forbidden construction tokens) and is CLEAN on the
//! project-bound shape, proves the request-name allow-list rejects a SECOND occurrence,
//! an occurrence in a different function, AND an occurrence placed after the
//! `configure_paths` closing brace but before the next function (the most-recent-`fn`
//! gap), and that the global scan is non-vacuous on EVERY scoped crate (each crate's
//! source set is non-empty and a probe token inserted into a representative file of
//! each crate would trip the scan).

use std::fs;
use std::path::{Path, PathBuf};

/// Repo root (two parents up from `crates/verter_session`).
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

/// Walk every `.rs` file rooted at `path` (recursively).
fn walk_rs_files(path: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(path) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_rs_files(&p, out);
        } else if p.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(p);
        }
    }
}

/// The external-TS PRODUCTION source directories scanned by this guard. Every
/// backend transport, the extension provider, the batch TSC checker, and the
/// type-runtime adapters are covered. `*_tests.rs` siblings (which legitimately
/// ASSERT the absence of the inferred knobs) are filtered out per file.
fn scoped_source_dirs() -> Vec<PathBuf> {
    let root = workspace_root();
    vec![
        root.join("crates").join("verter_lsp").join("src"),
        root.join("crates").join("verter_tsc").join("src"),
        root.join("crates").join("verter_type_runtime").join("src"),
    ]
}

/// PRODUCTION `.rs` source files across the scoped crates (excludes `*_tests.rs`).
fn production_sources() -> Vec<PathBuf> {
    let mut files = Vec::new();
    for dir in scoped_source_dirs() {
        walk_rs_files(&dir, &mut files);
    }
    files
        .into_iter()
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| !n.ends_with("_tests.rs"))
                .unwrap_or(true)
        })
        .collect()
}

/// PRODUCTION `.rs` source files under ONE scoped crate's `src/` (excludes
/// `*_tests.rs`). Used by the non-vacuity self-test to prove each crate has a
/// non-empty scanned source set.
fn production_sources_for_crate(crate_name: &str) -> Vec<PathBuf> {
    let dir = workspace_root().join("crates").join(crate_name).join("src");
    let mut files = Vec::new();
    walk_rs_files(&dir, &mut files);
    files
        .into_iter()
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| !n.ends_with("_tests.rs"))
                .unwrap_or(true)
        })
        .collect()
}

/// The forbidden inferred-project CONSTRUCTION / OPEN tokens. Each belongs ONLY to a
/// config-less inferred-project construction path (the legacy fallback shape), NEVER
/// to the project-bound contract. NONE is a bare `inferred`/`fallback` substring.
const FORBIDDEN_INFERRED_CONSTRUCTION_TOKENS: &[&str] = &[
    // tsserver: open a synthetic external project (the inferred/external open).
    "openExternalProject",
    // tsserver: the single-inferred-project knob.
    "useSingleInferredProject",
    // tsserver: per-project-root inferred-project knob.
    "useInferredProjectPerProjectRoot",
    // A synthetic inferred-project construction.
    "createInferredProject",
    // The alternate inferred-options key tsserver accepts for the same
    // construction (the request name `compilerOptionsForInferredProjects` is
    // separately allow-listed at its ONE config call site below).
    "inferredProjectCompilerOptions",
];

/// The ONE allow-listed `compilerOptionsForInferredProjects` CONFIGURATION call.
/// It CONFIGURES the compiler options tsserver applies to any inferred project (a
/// non-carrier, non-open setup call), so the request NAME is allowed — but ONLY at a
/// single live occurrence, and ONLY when that occurrence's byte offset sits inside the
/// brace-matched [`ALLOWLISTED_INFERRED_CONFIG_FN`] method BODY of
/// [`ALLOWLISTED_INFERRED_CONFIG_FILE`]. A SECOND live occurrence (anywhere), an
/// occurrence in a DIFFERENT function of that file, an occurrence in the bare impl scope
/// AFTER the method's closing brace, or ANY occurrence in another scoped file is a
/// violation — as is any occurrence of the construction tokens above.
const ALLOWLISTED_INFERRED_CONFIG_REQUEST: &str = "compilerOptionsForInferredProjects";
const ALLOWLISTED_INFERRED_CONFIG_FILE: &str = "crates/verter_lsp/src/extension_provider.rs";
/// The exact method whose brace-matched body may carry the single allow-listed
/// request-name occurrence — the `configure_paths` method on the
/// `TypeProvider for ExtensionTypeProvider` impl. The occurrence is the request name
/// passed to `self.query(...)` there; pinning it to this method's BODY SPAN (the
/// matched `{ … }`, not merely "the most-recent `fn` line above") is what stops an
/// inferred-open added elsewhere in the file — including in the bare impl scope just
/// after this method — from riding the allow-list.
const ALLOWLISTED_INFERRED_CONFIG_FN: &str = "configure_paths";
/// The impl-block discriminant: the `configure_paths` body is pinned to the method ON
/// the `TypeProvider for ExtensionTypeProvider` impl, so a same-named method on an
/// unrelated impl could not satisfy the allow-list.
const ALLOWLISTED_INFERRED_CONFIG_IMPL: &str = "TypeProvider for ExtensionTypeProvider";

/// A non-comment source line.
fn is_comment(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*')
}

/// The forbidden-token scan predicate for ONE line (excludes comments). Returns the
/// matched token if the line is a live (non-comment) occurrence.
fn forbidden_token_on_line(line: &str) -> Option<&'static str> {
    if is_comment(line) {
        return None;
    }
    FORBIDDEN_INFERRED_CONSTRUCTION_TOKENS
        .iter()
        .copied()
        .find(|tok| line.contains(tok))
}

/// The method name a `fn NAME(` declaration line introduces, if the (non-comment)
/// line is a function-signature line. Used both to LOCATE the `configure_paths`
/// signature (so the allow-list can brace-match its body) and to prove the
/// signature-parse handles every visibility form. Strips leading visibility / async /
/// `default` qualifiers in any order until the `fn ` keyword, so `fn configure_paths(`,
/// `pub fn`, `pub(crate) fn`, `pub(super) fn`, `async fn`, and `pub(crate) async fn`
/// all resolve.
fn fn_name_introduced_on_line(line: &str) -> Option<&str> {
    if is_comment(line) {
        return None;
    }
    let mut rest = line.trim_start();
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix("fn ") {
            let name_end = after
                .find(|c: char| !(c.is_alphanumeric() || c == '_'))
                .unwrap_or(after.len());
            let name = &after[..name_end];
            return (!name.is_empty()).then_some(name);
        }
        // Consume exactly one leading qualifier token; stop if none matches (the
        // line does not introduce a function). The longer `pub(...)` forms come
        // first so the bare `pub` prefix never shadows them.
        let next = [
            "pub(crate)",
            "pub(super)",
            "pub(self)",
            "pub",
            "async",
            "const",
            "unsafe",
            "extern",
            "default",
        ]
        .into_iter()
        .find_map(|kw| {
            rest.strip_prefix(kw)
                .filter(|tail| starts_with_boundary(tail))
        });
        match next {
            Some(tail) => rest = tail,
            None => return None,
        }
    }
}

/// `true` when `s` is empty or starts with a non-identifier byte — so stripping a
/// keyword prefix (`pub`, `async`) only matches the whole word, never a prefix of a
/// longer identifier (`pubish`, `asynchronicity`).
fn starts_with_boundary(s: &str) -> bool {
    s.chars()
        .next()
        .is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
}

/// The EXACT-SHAPE allow-list verdict for the request name in the allow-listed file's
/// source `src`: the request name is permitted at EXACTLY ONE live occurrence, and ONLY
/// when its BYTE OFFSET falls inside the brace-matched [`ALLOWLISTED_INFERRED_CONFIG_FN`]
/// method body (located on the [`ALLOWLISTED_INFERRED_CONFIG_IMPL`] impl). Returns a
/// violation string for every way the file deviates:
///   * the `configure_paths` body span cannot be located — the allow-list anchor is
///     stale (the method moved off the `TypeProvider for ExtensionTypeProvider` impl or
///     was renamed), so the guard would otherwise have no body to scope against;
///   * ZERO live occurrences inside that body — the legitimate config call vanished
///     (the guard would otherwise be vacuously clean), so the anchor is stale;
///   * MORE THAN ONE live occurrence inside the body — a second injection rode the
///     single-call allow;
///   * any live occurrence OUTSIDE the body span — an inferred-open / config injection
///     in a different function, OR in the bare impl scope after the method's closing
///     brace (the precise gap the previous most-recent-`fn` attribution missed).
fn allowlisted_request_violations(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let Some((body_open, body_close)) = configure_paths_body_span(src) else {
        out.push(format!(
            "{ALLOWLISTED_INFERRED_CONFIG_FILE}: could not locate the \
             `{ALLOWLISTED_INFERRED_CONFIG_FN}` method body on the \
             `{ALLOWLISTED_INFERRED_CONFIG_IMPL}` impl — the allow-list anchor is stale \
             (the method moved off the impl, was renamed, or its braces no longer match); \
             re-pin it"
        ));
        return out;
    };

    let occ = live_request_occurrences(src, ALLOWLISTED_INFERRED_CONFIG_REQUEST);
    let (inside, outside): (Vec<_>, Vec<_>) = occ
        .into_iter()
        .partition(|&(byte, _)| byte >= body_open && byte < body_close);

    match inside.len() {
        0 => out.push(format!(
            "{ALLOWLISTED_INFERRED_CONFIG_FILE}: expected EXACTLY ONE live \
             `{ALLOWLISTED_INFERRED_CONFIG_REQUEST}` occurrence inside the \
             `{ALLOWLISTED_INFERRED_CONFIG_FN}` method body, found NONE — the allow-list \
             anchor no longer matches the legitimate config call (it now lives elsewhere \
             or is absent); re-pin it"
        )),
        1 => {}
        n => out.push(format!(
            "{ALLOWLISTED_INFERRED_CONFIG_FILE}: expected EXACTLY ONE live \
             `{ALLOWLISTED_INFERRED_CONFIG_REQUEST}` occurrence inside the \
             `{ALLOWLISTED_INFERRED_CONFIG_FN}` body, found {n} (lines {:?}) — a second \
             occurrence must not ride the single-call allow-list",
            inside.iter().map(|(_, ln)| *ln).collect::<Vec<_>>()
        )),
    }
    for (_byte, lineno) in &outside {
        out.push(format!(
            "{ALLOWLISTED_INFERRED_CONFIG_FILE}:{lineno}: \
             `{ALLOWLISTED_INFERRED_CONFIG_REQUEST}` occurs OUTSIDE the \
             `{ALLOWLISTED_INFERRED_CONFIG_FN}` method body (a different function, or the \
             bare impl scope after its closing brace) — only the in-body configure_paths \
             config call may carry the request name"
        ));
    }
    out
}

/// Every LIVE (non-comment) occurrence of `needle` in `src`, as
/// `(byte_offset, lineno_1based)`. Byte offsets are absolute into `src`, so the caller
/// can test membership in a brace-matched body span. Comment lines are skipped wholesale
/// (final-state prose naming the request is not a live occurrence).
fn live_request_occurrences(src: &str, needle: &str) -> Vec<(usize, usize)> {
    let mut hits = Vec::new();
    let mut line_start = 0usize;
    for (idx, line) in src.split_inclusive('\n').enumerate() {
        if !is_comment(line) {
            // Record every match start byte on this line (there can be more than one).
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(needle) {
                hits.push((line_start + search_from + rel, idx + 1));
                search_from += rel + needle.len();
            }
        }
        line_start += line.len();
    }
    hits
}

/// Rewrite `src` so every line-comment, block-comment, string-literal, and char-literal
/// byte becomes a space (newlines preserved), leaving only CODE bytes. The brace scanner
/// runs over this view so a `{`/`}` inside a string (`json!({ "options": … })`), char
/// (`'{'`), or comment never miscounts the body braces. Lifetime ticks (`'a`, `'_`) are
/// preserved in place (they are not char literals), so a `<'_>` in a signature does not
/// swallow following code.
fn blank_noncode(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = vec![b' '; bytes.len()];
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'\n' | b'\r' | b'\t' => {
                out[i] = b;
                i += 1;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            b'"' => {
                i += 1;
                while i < bytes.len() {
                    match bytes[i] {
                        b'\\' => i += 2,
                        b'"' => {
                            i += 1;
                            break;
                        }
                        b'\n' => {
                            out[i] = b'\n';
                            i += 1;
                        }
                        _ => i += 1,
                    }
                }
            }
            b'\'' => {
                // A char literal is a tick, one (optionally escaped) char, then a
                // closing tick within ≤4 bytes. A lifetime (`'a`/`'_`/`'static`) is a
                // tick followed by an identifier with NO closing tick — leave it in
                // place so it does not blank following code.
                let mut j = i + 1;
                if j < bytes.len() && bytes[j] == b'\\' {
                    j += 2;
                } else if j < bytes.len() {
                    j += 1;
                }
                if j < bytes.len() && bytes[j] == b'\'' {
                    i = j + 1;
                } else {
                    out[i] = b'\'';
                    i += 1;
                }
            }
            _ => {
                out[i] = b;
                i += 1;
            }
        }
    }
    String::from_utf8(out).expect("ascii-preserving rewrite")
}

/// Byte span `[open_brace_inclusive, close_brace_exclusive)` of the
/// [`ALLOWLISTED_INFERRED_CONFIG_FN`] method body, located by: (1) tracking entry into
/// the [`ALLOWLISTED_INFERRED_CONFIG_IMPL`] impl block by its signature line + brace
/// depth, (2) finding the `configure_paths` fn signature WHILE inside that impl,
/// (3) scanning to its opening `{`, (4) brace-matching to the close. The brace scan runs
/// over [`blank_noncode`] so literal / comment braces never throw off the count. Returns
/// `None` when no such method body exists (a stale anchor).
fn configure_paths_body_span(src: &str) -> Option<(usize, usize)> {
    let code = blank_noncode(src);
    let code_bytes = code.as_bytes();
    let mut line_start = 0usize;
    let mut in_target_impl_depth: Option<usize> = None;
    let mut brace_depth = 0usize;
    let mut found_sig_open: Option<usize> = None;

    for line in src.split_inclusive('\n') {
        let line_code = &code[line_start..line_start + line.len()];
        let trimmed = line.trim_start();

        // Enter the target impl when its signature line appears (before counting this
        // line's braces, so the impl's own opening `{` is recorded at the right depth).
        if trimmed.starts_with("impl") && line.contains(ALLOWLISTED_INFERRED_CONFIG_IMPL) {
            in_target_impl_depth = Some(brace_depth);
        }

        // While inside the target impl, the first `configure_paths` signature pins the
        // body's opening brace (the `{` at or after the signature in the code view).
        if found_sig_open.is_none()
            && in_target_impl_depth.is_some()
            && fn_name_introduced_on_line(line) == Some(ALLOWLISTED_INFERRED_CONFIG_FN)
        {
            // The signature may carry a multi-line return type before `{`; scan the
            // whole remaining code view from the signature's line start.
            if let Some(rel) = code[line_start..].find('{') {
                found_sig_open = Some(line_start + rel);
            }
        }

        // Track running brace depth from this line's CODE braces; drop the impl marker
        // when the impl's own block closes.
        for &ch in line_code.as_bytes() {
            match ch {
                b'{' => brace_depth += 1,
                b'}' => {
                    brace_depth = brace_depth.saturating_sub(1);
                    if in_target_impl_depth == Some(brace_depth) {
                        in_target_impl_depth = None;
                    }
                }
                _ => {}
            }
        }

        line_start += line.len();
    }

    let open = found_sig_open?;
    // Brace-match from the body's opening `{` to its close over the code view.
    let mut depth = 0usize;
    let mut idx = open;
    while idx < code_bytes.len() {
        match code_bytes[idx] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((open, idx));
                }
            }
            _ => {}
        }
        idx += 1;
    }
    None
}

#[test]
fn no_fallback_to_inferred_anywhere() {
    let root = workspace_root();
    let sources = production_sources();
    assert!(
        !sources.is_empty(),
        "the scoped source enumeration is empty — the guard is vacuous (broken paths?)"
    );

    let mut violations: Vec<String> = Vec::new();

    for file in &sources {
        let content = match fs::read_to_string(file) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(file)
            .display()
            .to_string()
            .replace('\\', "/");

        for (lineno, line) in content.lines().enumerate() {
            // The inferred-project CONSTRUCTION / OPEN tokens are banned everywhere.
            if let Some(tok) = forbidden_token_on_line(line) {
                violations.push(format!(
                    "{rel}:{}: forbidden inferred-project construction knob `{tok}`: {}",
                    lineno + 1,
                    line.trim()
                ));
            }

            // The `compilerOptionsForInferredProjects` request NAME is allowed ONLY
            // in the single allow-listed file (and there, only inside `configure_paths`
            // — enforced exact-shape after this loop). Any live occurrence in ANOTHER
            // scoped file is a reintroduced inferred-options injection.
            if !is_comment(line)
                && line.contains(ALLOWLISTED_INFERRED_CONFIG_REQUEST)
                && rel != ALLOWLISTED_INFERRED_CONFIG_FILE
            {
                violations.push(format!(
                    "{rel}:{}: `{ALLOWLISTED_INFERRED_CONFIG_REQUEST}` request outside the ONE \
                     allow-listed config call ({ALLOWLISTED_INFERRED_CONFIG_FILE}::\
                     {ALLOWLISTED_INFERRED_CONFIG_FN}): {}",
                    lineno + 1,
                    line.trim()
                ));
            }

            // tsserver must NOT define a `configure_paths` IMPL (the inferred
            // `paths`/`baseUrl` injection method — its trait DEFAULT no-op stays;
            // only the tsserver-specific override is forbidden). Scope this to the
            // tsserver transport source so the ExtensionTypeProvider's legitimate
            // `configure_paths` override (a different provider, NOT inferred-carrier
            // construction) is not falsely flagged.
            if !is_comment(line)
                && line.trim_start().starts_with("fn configure_paths")
                && rel.starts_with("crates/verter_type_runtime/src/tsserver/")
            {
                violations.push(format!(
                    "{rel}:{}: the tsserver transport must NOT define a `configure_paths` impl \
                     (the inferred paths/baseUrl injection) — it inherits the trait default \
                     no-op: {}",
                    lineno + 1,
                    line.trim()
                ));
            }
        }
    }

    // EXACT-SHAPE allow-list for the request name in the allow-listed file: it is
    // permitted at EXACTLY ONE live occurrence, and ONLY inside the `configure_paths`
    // method body. A second occurrence (anywhere in the file) or an occurrence in any
    // other function is a violation — so an inferred-open / config injection added
    // elsewhere in this file is caught, not hidden behind a file-wide pass.
    let allowlisted_rs = root.join(ALLOWLISTED_INFERRED_CONFIG_FILE);
    let allowlisted_src = fs::read_to_string(&allowlisted_rs)
        .unwrap_or_else(|e| panic!("read {}: {e}", allowlisted_rs.display()));
    violations.extend(allowlisted_request_violations(&allowlisted_src));

    // tsgo POSITIVE assertion: the owned `--api` carrier path reaches configured
    // membership through `open_project`, never an inferred/single-file fallback.
    // The owned resolution body (`crates/verter_type_runtime/src/tsgo/owned.rs`)
    // MUST carry all three project-bound witnesses; their absence means the
    // project-bound membership was replaced by a config-less open.
    let owned_rs = root.join("crates/verter_type_runtime/src/tsgo/owned.rs");
    let owned_src = fs::read_to_string(&owned_rs)
        .unwrap_or_else(|e| panic!("read {}: {e}", owned_rs.display()));
    for witness in [
        "update_snapshot_open_project",
        "project_for_config",
        "root_files",
    ] {
        let present = owned_src
            .lines()
            .any(|line| !is_comment(line) && line.contains(witness));
        if !present {
            violations.push(format!(
                "crates/verter_type_runtime/src/tsgo/owned.rs: the owned tsgo carrier resolution \
                 lost its project-bound membership witness `{witness}` — membership must reach \
                 the configured project via `open_project` + `project.root_files`, never an \
                 inferred/single-file open"
            ));
        }
    }

    // tsgo STRUCTURAL invariant: NO config-less owned restart backend exists. The
    // owned startup wraps only via the project-bound `new_owned`; the config-less
    // `struct TsgoBackend` / its `ResilientBackend` impl / the config-less
    // `pub fn new(` / the `tsgo_resilient::new(` wrap are ABSENT from the source.
    let main_rs = root.join("crates/verter_lsp/src/main.rs");
    let resilient_rs = root.join("crates/verter_lsp/src/tsgo/resilient.rs");
    let main_src =
        fs::read_to_string(&main_rs).unwrap_or_else(|e| panic!("read {}: {e}", main_rs.display()));
    let resilient_src = fs::read_to_string(&resilient_rs)
        .unwrap_or_else(|e| panic!("read {}: {e}", resilient_rs.display()));

    // main.rs must CALL `require_owned_tsconfig` (the explicit-binding resolver) on
    // the owned-startup path — not merely define the helper.
    if !calls_require_owned_tsconfig(&main_src) {
        violations.push(
            "crates/verter_lsp/src/main.rs: owned tsgo startup must CALL `require_owned_tsconfig` \
             (the explicit-binding resolver) — no live call site found (a config-less owned \
             startup would skip it)"
                .to_string(),
        );
    }
    for (needle, why) in [
        (
            "struct TsgoBackend",
            "the config-less owned restart backend struct must be ABSENT (only the \
             project-bound owned backend exists)",
        ),
        (
            "impl ResilientBackend<TsgoTypeProvider> for TsgoBackend",
            "the config-less owned restart impl must be ABSENT",
        ),
        (
            "pub fn new(",
            "the config-less `pub fn new(` resilient constructor must be ABSENT — owned \
             startup uses only `new_owned`",
        ),
    ] {
        if has_live(&resilient_src, needle) {
            violations.push(format!("crates/verter_lsp/src/tsgo/resilient.rs: {why}"));
        }
    }
    if has_live(&main_src, "tsgo_resilient::new(") {
        violations.push(
            "crates/verter_lsp/src/main.rs: the config-less `tsgo_resilient::new(` wrap must be \
             ABSENT — owned startup wraps only via `tsgo_resilient::new_owned(`"
                .to_string(),
        );
    }

    assert!(
        violations.is_empty(),
        "inferred-project fallback/construction found — the external-TS engine is project-bound \
         on EVERY backend (the carrier is a real configured-project member):\n{}",
        violations.join("\n")
    );
}

/// Does a non-comment line of `src` contain `needle`?
fn has_live(src: &str, needle: &str) -> bool {
    src.lines()
        .any(|line| !is_comment(line) && line.contains(needle))
}

/// Does a non-comment line CALL `require_owned_tsconfig` (`require_owned_tsconfig(`),
/// as opposed to merely DEFINING it (`fn require_owned_tsconfig(`)?
fn calls_require_owned_tsconfig(src: &str) -> bool {
    src.lines().any(|line| {
        if is_comment(line) {
            return false;
        }
        let is_definition = line.contains("fn require_owned_tsconfig(");
        !is_definition && line.contains("require_owned_tsconfig(")
    })
}

/// DISCRIMINATING self-test: each predicate FIRES on a synthetic config-less
/// construction (the forbidden construction tokens) and is CLEAN on the project-bound
/// shape. Proves the guard FAILS on the config-less inferred shape and is non-vacuous.
#[test]
fn no_fallback_to_inferred_anywhere_self_test_discriminates() {
    // ── forbidden construction tokens FIRE on a live line, CLEAN on a comment ──
    assert_eq!(
        forbidden_token_on_line("        client.request(\"openExternalProject\", args).await"),
        Some("openExternalProject"),
        "a live openExternalProject call must trip the guard"
    );
    assert_eq!(
        forbidden_token_on_line("    \"useSingleInferredProject\": true,"),
        Some("useSingleInferredProject"),
        "a live useSingleInferredProject knob must trip the guard"
    );
    assert_eq!(
        forbidden_token_on_line("    let p = self.createInferredProject(root);"),
        Some("createInferredProject"),
        "a live createInferredProject construction must trip the guard"
    );
    assert_eq!(
        forbidden_token_on_line("        \"inferredProjectCompilerOptions\": options,"),
        Some("inferredProjectCompilerOptions"),
        "a live inferredProjectCompilerOptions injection must trip the guard"
    );
    // A comment NAMING a forbidden token (final-state prose) is NOT a violation.
    assert_eq!(
        forbidden_token_on_line("    // openExternalProject is forbidden — project-bound only."),
        None,
        "a comment naming a forbidden construction token must NOT trip the guard"
    );
    assert_eq!(
        forbidden_token_on_line("        * useSingleInferredProject is forbidden here."),
        None,
        "a block-comment continuation line must NOT trip the guard"
    );

    // ── bare `inferred`/`fallback` substrings must NOT be construction tokens ──
    // (the legitimate workspace precedence-tier name + the vite-config concept).
    assert_eq!(
        forbidden_token_on_line("    let rank = ProjectRank::Inferred;"),
        None,
        "ProjectRank::Inferred (a workspace precedence tier) must NOT trip the guard"
    );
    assert_eq!(
        forbidden_token_on_line("    ProjectPayload::Fallback { .. } => None,"),
        None,
        "ProjectPayload::Fallback (a vite-config workspace concept) must NOT trip the guard"
    );
    assert_eq!(
        forbidden_token_on_line("    let range = document_symbol_fallback_range(node);"),
        None,
        "a feature-level fallback (document-symbol fallback range) must NOT trip the guard"
    );

    // ── the `configure_paths` impl predicate (tsserver scope) ──
    assert!(
        "    fn configure_paths(&self, base_url: &str, paths: Value) -> Fut {"
            .trim_start()
            .starts_with("fn configure_paths"),
        "a tsserver configure_paths impl signature must match the impl predicate"
    );

    // ── `struct TsgoBackend` vs the project-bound owned backend ──
    assert!(
        has_live("struct TsgoBackend {", "struct TsgoBackend"),
        "the config-less backend struct must trip the guard"
    );
    assert!(
        !has_live("struct TsgoOwnedBackend {", "struct TsgoBackend"),
        "the project-bound owned backend must NOT trip the config-less needle"
    );

    // ── `pub fn new(` vs `pub fn new_owned(` ──
    assert!(
        has_live("    pub fn new(", "pub fn new("),
        "the config-less constructor must trip the guard"
    );
    assert!(
        !has_live("    pub fn new_owned(", "pub fn new("),
        "the project-bound `new_owned` must NOT trip the `pub fn new(` needle"
    );

    // ── `tsgo_resilient::new(` vs `tsgo_resilient::new_owned(` ──
    assert!(
        has_live("let r = tsgo_resilient::new(", "tsgo_resilient::new("),
        "the config-less wrap must trip the guard"
    );
    assert!(
        !has_live("let r = tsgo_resilient::new_owned(", "tsgo_resilient::new("),
        "the project-bound `new_owned` wrap must NOT trip the `tsgo_resilient::new(` needle"
    );

    // ── `require_owned_tsconfig` CALL vs definition ──
    assert!(
        calls_require_owned_tsconfig("    let cfg = require_owned_tsconfig(root)?;"),
        "a real require_owned_tsconfig call must read as CALLED"
    );
    assert!(
        !calls_require_owned_tsconfig(
            "fn require_owned_tsconfig(root: &Path) -> Result<String, String> {"
        ),
        "the definition alone must read as NOT CALLED"
    );

    // ── EXACT-SHAPE request-name allow-list: pinned by BRACE/BODY SCOPE to ONE
    //    occurrence inside the `configure_paths` method body, rejecting a second
    //    occurrence, an off-function one, AND the most-recent-`fn` gap (an occurrence
    //    after the method's closing brace but before the next fn). ──

    // `fn_name_introduced_on_line` resolves every visibility/async form and ignores
    // non-signature lines — this locator must handle `pub(super) fn`/`pub(crate) async
    // fn` so the body-span scan finds `configure_paths` whatever its visibility.
    assert_eq!(
        fn_name_introduced_on_line(
            "    fn configure_paths(&self, base_url: &str, paths: Value) -> Fut {"
        ),
        Some("configure_paths"),
        "a trait-impl configure_paths signature must resolve its fn name"
    );
    assert_eq!(
        fn_name_introduced_on_line("    async fn query(&self, method: &str) -> Value {"),
        Some("query"),
        "an `async fn` signature must resolve its fn name"
    );
    assert_eq!(
        fn_name_introduced_on_line("    pub fn new_owned(cfg: Cfg) -> Self {"),
        Some("new_owned"),
        "a `pub fn` signature must resolve its fn name"
    );
    assert_eq!(
        fn_name_introduced_on_line("    pub(super) fn configure_paths(&self) -> Fut {"),
        Some("configure_paths"),
        "a `pub(super) fn` signature must resolve its fn name"
    );
    assert_eq!(
        fn_name_introduced_on_line("    pub(crate) async fn configure_paths(&self) -> Fut {"),
        Some("configure_paths"),
        "a `pub(crate) async fn` signature must resolve its fn name"
    );
    assert_eq!(
        fn_name_introduced_on_line("        let x = self.query(\"foo\", v).await;"),
        None,
        "a non-signature line introduces no function"
    );
    // The keyword strip only matches whole words (no `pubish`/`asynchronicity` prefix).
    assert_eq!(
        fn_name_introduced_on_line("    pubish_fn_like_ident = 1;"),
        None,
        "a `pub`-prefixed identifier must NOT be read as a `pub fn`"
    );

    // `blank_noncode` blanks string / char / comment bytes (newlines preserved) so the
    // brace scanner never miscounts a `{` inside a string literal or a comment.
    let blanked = blank_noncode("let s = \"a { b }\"; // } trailing\nlet t = '{';\n");
    assert!(
        !blanked.contains('{') && !blanked.contains('}'),
        "blank_noncode must remove braces inside strings/chars/comments; got: {blanked:?}"
    );
    assert_eq!(
        blanked.len(),
        "let s = \"a { b }\"; // } trailing\nlet t = '{';\n".len(),
        "blank_noncode must preserve byte length (offset-preserving rewrite)"
    );
    // A lifetime tick is NOT a char literal — it must stay in place so it does not
    // swallow following code braces.
    let lt = blank_noncode("fn f<'a>(x: &'a u8) { let y = 1; }\n");
    assert!(
        lt.contains('{') && lt.contains('}'),
        "blank_noncode must keep code braces after a lifetime tick; got: {lt:?}"
    );

    // Helper: wrap a method body in the target impl so `configure_paths_body_span` has
    // an impl + signature to anchor on (mirrors the real file's structure).
    let in_impl = |body: &str| -> String {
        format!(
            "impl<T: TsQueryTransport> TypeProvider for ExtensionTypeProvider<T> {{\n{body}}}\n"
        )
    };

    // The REAL allow-listed shape: exactly one request-name occurrence, inside the
    // brace-matched `configure_paths` body, is CLEAN.
    let legit = in_impl(
        "    fn configure_paths(&self) {\n        let _ = self\n            .query(\n            \
         \"compilerOptionsForInferredProjects\",\n                json!({}),\n            \
         )\n            .await;\n    }\n",
    );
    assert!(
        allowlisted_request_violations(&legit).is_empty(),
        "the single configure_paths config call must be CLEAN; got: {:?}",
        allowlisted_request_violations(&legit)
    );

    // A SECOND occurrence (both inside the body) trips the exactly-one rule — a real
    // inferred-options injection added next to the legit call.
    let two = in_impl(
        "    fn configure_paths(&self) {\n        \
         q(\"compilerOptionsForInferredProjects\", a);\n        \
         q(\"compilerOptionsForInferredProjects\", b);\n    }\n",
    );
    let two_v = allowlisted_request_violations(&two);
    assert!(
        two_v.iter().any(|m| m.contains("found 2")),
        "a second request-name occurrence must trip the exactly-one allow-list; got: {two_v:?}"
    );

    // An occurrence in a DIFFERENT function of the same impl (an inferred-open added
    // elsewhere) is rejected even though it is a single occurrence — it falls OUTSIDE
    // the configure_paths body span.
    let elsewhere = in_impl(
        "    fn configure_paths(&self) {}\n    fn open_file(&self, p: &str) {\n        \
         q(\"compilerOptionsForInferredProjects\", opts);\n    }\n",
    );
    let elsewhere_v = allowlisted_request_violations(&elsewhere);
    assert!(
        elsewhere_v.iter().any(|m| m.contains("OUTSIDE"))
            && elsewhere_v.iter().any(|m| m.contains("found NONE")),
        "a request-name occurrence in another function must be rejected as OUTSIDE the body \
         (and the empty body flagged stale); got: {elsewhere_v:?}"
    );

    // THE BARE-IMPL-SCOPE GAP: a single occurrence placed in the bare impl scope AFTER
    // `configure_paths`'s closing brace but BEFORE the next fn. A most-recent-`fn`
    // attribution wrongly accepts this (count==1, enclosing == configure_paths); the
    // brace-matched body span REJECTS it as OUTSIDE the body.
    let after_close = in_impl(
        "    fn configure_paths(&self) {\n        let opts = json!({ \"k\": 1 });\n    }\n\n    \
         const STRAY: &str = \"compilerOptionsForInferredProjects\";\n\n    \
         fn update_workspace_folders(&self) {}\n",
    );
    let after_close_v = allowlisted_request_violations(&after_close);
    assert!(
        after_close_v.iter().any(|m| m.contains("OUTSIDE"))
            && after_close_v.iter().any(|m| m.contains("found NONE")),
        "an occurrence after the configure_paths closing brace (before the next fn) must be \
         REJECTED as OUTSIDE the body — the most-recent-`fn` gap; got: {after_close_v:?}"
    );

    // ZERO occurrences inside the body flags the allow-list anchor as stale.
    let none_v = allowlisted_request_violations(&in_impl("    fn configure_paths(&self) {}\n"));
    assert!(
        none_v.iter().any(|m| m.contains("found NONE")),
        "a missing request-name occurrence must flag the anchor as stale; got: {none_v:?}"
    );

    // A STALE ANCHOR: no `configure_paths` method on the target impl at all (renamed or
    // moved off the impl) is flagged — the body span cannot be located.
    let stale = "impl<T> TypeProvider for ExtensionTypeProvider<T> {\n    fn other(&self) {}\n}\n";
    let stale_v = allowlisted_request_violations(stale);
    assert!(
        stale_v.iter().any(|m| m.contains("could not locate")),
        "a missing configure_paths body must flag the anchor as stale; got: {stale_v:?}"
    );

    // `configure_paths_body_span` brace-matches over literal braces: the body contains
    // `json!({ … })` (nested code braces) AND a `"{"`-bearing string, yet the span ends
    // at the method's true closing brace, not a literal one.
    let nested = in_impl(
        "    fn configure_paths(&self) {\n        let _ = json!({ \"a\": \"}\" });\n        \
         let _ = \"compilerOptionsForInferredProjects\";\n    }\n",
    );
    let (open, close) = configure_paths_body_span(&nested).expect("nested body span");
    let occ_byte = nested
        .find("\"compilerOptionsForInferredProjects\"")
        .unwrap();
    assert!(
        occ_byte > open && occ_byte < close,
        "the request occurrence inside a body with nested/literal braces must fall within the \
         matched span ({open},{close}); occ at {occ_byte}"
    );
    assert!(
        allowlisted_request_violations(&nested).is_empty(),
        "a body with nested code braces + a literal brace string must still be CLEAN; got: {:?}",
        allowlisted_request_violations(&nested)
    );

    // The ACTUAL extension_provider.rs source satisfies the exact shape (the live
    // anchor is real, not a stale literal) — proves the post-loop check is non-vacuous.
    let live_provider = fs::read_to_string(workspace_root().join(ALLOWLISTED_INFERRED_CONFIG_FILE))
        .expect("read the live extension_provider.rs");
    assert!(
        allowlisted_request_violations(&live_provider).is_empty(),
        "the live {ALLOWLISTED_INFERRED_CONFIG_FILE} must satisfy the exact-shape allow-list; \
         got: {:?}",
        allowlisted_request_violations(&live_provider)
    );
    // And the live body span is real (non-empty), so the brace-scope pin is anchored on
    // an actual method, not vacuously absent.
    assert!(
        configure_paths_body_span(&live_provider).is_some_and(|(o, c)| c > o),
        "the live extension_provider.rs must expose a real configure_paths body span"
    );

    // ── NON-VACUITY: every scoped crate has a non-empty production source set, so
    //    the global scan actually covers all three (a broken path that emptied a
    //    crate's set would make the scan vacuous for it). ──
    for crate_name in ["verter_lsp", "verter_tsc", "verter_type_runtime"] {
        let crate_sources = production_sources_for_crate(crate_name);
        assert!(
            !crate_sources.is_empty(),
            "scoped crate `{crate_name}` has an empty production source set — the global scan \
             would be vacuous for it"
        );
        // And a synthetic reintroduction in a representative file of that crate
        // WOULD be caught: prove the scan predicate is applied per crate by
        // confirming each crate's set is reachable from the same predicate.
        let probe_line = "        client.request(\"openExternalProject\", v).await;";
        assert_eq!(
            forbidden_token_on_line(probe_line),
            Some("openExternalProject"),
            "the per-crate scan applies the same forbidden-token predicate"
        );
    }
}
