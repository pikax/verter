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
//!   * tsgo (owned) — the OWNED startup is PER-PROJECT BOUND (REOPEN-A). It no longer
//!     gates on a root `<workspace>/tsconfig.json`; startup only requires that AT LEAST
//!     ONE configured project exist anywhere (the bounded
//!     `has_configured_ts_project_anywhere` spawn precondition, accepting
//!     `packages/*/tsconfig.json` monorepos), and the carrier's OWN owning configured
//!     project is resolved PER QUERY. Every production OWNED carrier diagnostics
//!     request obtains a `BoundProject` from the SHARED project-binding helper
//!     (`crates/verter_lsp/src/tsgo/project_binding.rs`: published snapshot →
//!     `WorkspaceProjectResolver` → `ProjectBinding` → `ensure_project` →
//!     `BoundProject`) BEFORE delegating to `TsgoOwnedProvider::get_diagnostics`;
//!     `NoProject`, `Ambiguous`, `SyntheticScratch`, and a pre-published snapshot each
//!     return NO external-TS diagnostics (fail closed, never a `tsgo --lsp` inferred /
//!     own-discovery fall-through). This gate is source→owning-project ADMISSION (a
//!     `BoundProject` witness EXISTS), NOT `root_files`-membership / project-bound
//!     EXECUTION on the served diagnostics: an ADMITTED carrier's diagnostics ride
//!     OWNED's `--lsp` pull's OWN configured-project discovery. The `--api` checker's
//!     per-query membership witnesses (`update_snapshot_open_project` /
//!     `project_for_config` / `root_files`, supplied the tsconfig PER QUERY) are
//!     retained on the TEST-ONLY typecheck oracle, not the production diagnostics
//!     surface. The config-less owned
//!     restart backend (`struct TsgoBackend` + its `ResilientBackend` impl + the
//!     config-less `pub fn new(` constructor + a `tsgo_resilient::new(` wrap) is ABSENT
//!     — only the project-bound owned backend exists. The test-only
//!     `semantic_diagnostics_for_carrier_in_project` `--api` oracle is NOT a production guard
//!     target. Carrier TS FEATURE queries are LIKEWISE gated on the resolved
//!     `BoundProject`: every carrier feature method on `TsgoCompositeProvider` routes
//!     through the `feature_admits` admission helper (memoized by the generation-scoped
//!     `CarrierAdmissionCache`) BEFORE delegating to OWNED, so a non-bound carrier serves
//!     the empty/none external default, never a `--lsp` self-discovery fall-through — the
//!     `carrier_features_are_admission_gated` tail test below pins that shape exhaustively
//!     against the `ProviderFeature` registry.
//!
//! This STATIC guard is the GLOBAL source-level backstop for the project-bound
//! membership contract. It walks the external-TS PRODUCTION source across
//! `crates/verter_lsp/src/**`, `crates/verter_tsc/src/**`, and
//! `crates/verter_type_runtime/src/**` (BOTH the tsserver and the tsgo backend
//! paths), EXCLUDING `*_tests.rs` siblings and comment lines, and FAILS if any
//! inferred-project CONSTRUCTION / OPEN knob appears anywhere, AND asserts the
//! per-project OWNED binding shape (points below).
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
//! ## The per-project OWNED binding rule-set (REOPEN-A)
//!
//! Beyond the inferred-construction scan, the guard asserts ALL SIX:
//!   1. the OWNED carrier-diagnostics production path (`composite.rs`) obtains a
//!      `BoundProject` from the shared project-binding helper before delegating —
//!      witnessed by a live `resolve_carrier_bound(` CALL (call-shape, not a bare mention
//!      or the helper's definition) that is BRACE-SCOPED to the gate method body: it must
//!      occur INSIDE the `diagnostics_gated` method on `impl TsgoCompositeProvider`
//!      (byte-offset within the brace-matched body span), not merely somewhere in
//!      `composite.rs`, so a decoy `resolve_carrier_bound(` elsewhere in the file no
//!      longer satisfies REQ-1. A `carrier_source_of(` companion-classify call stays
//!      FILE-LEVEL (its gating role is the same body, but the primary REQ-1/REQ-6
//!      witnesses are the two brace-scoped calls), and the `published_root` →
//!      `WorkspaceProjectResolver` → `ensure_project` → `BoundProject` chain lives in
//!      `project_binding.rs`. As a best-effort static proxy for "fails closed in the
//!      gate", the fail-closed empty-result return (`Ok(Vec::new())`) is asserted PRESENT
//!      inside the same gate body span (its non-bound-`else`-arm dominance is NOT
//!      re-derived statically — see below). This STATIC guard asserts the gate's
//!      source-level SHAPE; the RUNTIME fail-closed control-flow (a non-bound carrier
//!      yields NO external-TS diagnostics, resolved BEFORE delegation, never an OWNED
//!      `--lsp` fall-through) is proven separately by the discriminating tests in
//!      `crates/verter_lsp/tests/owned_binding_gate.rs`
//!      (`every_non_bound_carrier_binding_variant_is_fail_closed_none`,
//!      `gate_no_project_carrier_fails_closed_to_empty`,
//!      `gate_non_bound_carrier_fails_closed_to_empty_background`,
//!      `gate_bound_carrier_delegates_to_owned`) — the static+runtime split is deliberate:
//!      the static guard is the source-shape backstop, the runtime tests own dominance;
//!   2. the tsgo membership witnesses remain in `owned.rs`
//!      (`update_snapshot_open_project` / `project_for_config` / `root_files`);
//!   3. a startup configured-project-presence spawn gate exists — a live CALL to
//!      `has_configured_ts_project_anywhere` in `main.rs`;
//!   4. ABSENCE of a `require_owned_tsconfig` call/def, a root-only
//!      `workspace_root.join("tsconfig.json")` OWNED gate, a stored single
//!      `tsconfig_path` in the owned provider (`owned.rs`) / backend (`resilient.rs`),
//!      and a `new_owned(...tsconfig_path...)` param;
//!   5. the inferred-construction-token bans + the scoped
//!      `compilerOptionsForInferredProjects` allow-list stay UNCHANGED;
//!   6. no raw path-only OWNED open that bypasses the witness — the gate checks the
//!      resolved `BoundProject` via a live `into_bound(` call that is likewise
//!      BRACE-SCOPED to the `diagnostics_gated` gate body (a decoy `into_bound` elsewhere
//!      in `composite.rs` does NOT satisfy REQ-6) before delegating, AND the test-only
//!      OWNED `--api` oracle `semantic_diagnostics_for_carrier_in_project` is NEVER CALLED
//!      from PRODUCTION source (its DEFINITION in `owned.rs` and its TEST call sites are
//!      allowed; a production `src` call would be a raw path-only OWNED diagnostics route
//!      bypassing the always-present `BoundProject` admission layer).
//!
//! DISCRIMINATING: the self-test below proves every predicate FIRES on a synthetic
//! reintroduction (the forbidden construction tokens; a reintroduced
//! `require_owned_tsconfig` call / root-only `join` / stored `tsconfig_path` field) and
//! is CLEAN on the project-bound per-project-binding shape, proves the CALL-shape
//! predicates (`resolve_carrier_bound(` / `carrier_source_of(` / `into_bound(` /
//! `has_configured_ts_project_anywhere(`) FIRE on a live call yet stay CLEAN on the bare
//! DEFINITION and a comment, proves the BRACE-SCOPED gate predicates
//! (`calls_named_fn_in_span` / `live_needle_in_span` over `composite_gate_body_span`) are
//! satisfied by the real IN-BODY `resolve_carrier_bound(` / `into_bound` / `Ok(Vec::new())`
//! yet are NOT satisfied by a decoy of each placed in a SIBLING method OUTSIDE the gate
//! body (where the retired file-level `calls_named_fn` would be fooled), that the
//! gate-body-span locator handles the real method shape (async, multi-line signature,
//! nested `else { … }` + literal-brace strings via `blank_noncode`) and reports a stale
//! anchor when `diagnostics_gated` is absent, and that the live `composite.rs` exposes a
//! real non-empty gate span carrying all three in-body witnesses, proves the PRODUCTION
//! oracle-absence check FIRES on a
//! synthetic production call to `semantic_diagnostics_for_carrier_in_project` yet is CLEAN
//! on its definition and a comment, proves the `new_owned(` arg-list check FIRES on a
//! (single- OR multi-line) `tsconfig_path` argument yet is CLEAN without one and does not
//! match a longer callee at a non-identifier boundary, proves the request-name allow-list
//! rejects a SECOND occurrence, an occurrence in a different function, AND an occurrence
//! placed after the `configure_paths` closing brace but before the next function (the
//! most-recent-`fn` gap), and that the global scan is non-vacuous on EVERY scoped crate
//! (each crate's source set is non-empty and a probe token inserted into a representative
//! file of each crate would trip the scan).

use std::collections::BTreeSet;
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

/// The OWNED carrier-diagnostics gate: the `diagnostics_gated` method on the inherent
/// `impl TsgoCompositeProvider` in composite.rs, whose brace-matched BODY must carry the
/// REQ-1/REQ-6 binding witnesses (`resolve_carrier_bound(` + `into_bound`) AND the
/// fail-closed empty-result return. The inherent impl declares `diagnostics_gated`
/// (unique in the file) and precedes the trait impls, so the impl-name discriminant
/// resolves to it. Pinning the two binding CALLS to THIS body span — not merely
/// "somewhere in composite.rs" — is what stops a decoy `resolve_carrier_bound(` /
/// `into_bound` elsewhere in the file from satisfying REQ-1/REQ-6.
const OWNED_GATE_FILE: &str = "crates/verter_lsp/src/tsgo/composite.rs";
const OWNED_GATE_FN: &str = "diagnostics_gated";
const OWNED_GATE_IMPL: &str = "TsgoCompositeProvider";
/// The fail-closed no-external-TS-diagnostics return the gate takes on a NON-bound
/// carrier (the `into_bound()` None arm). Asserted PRESENT inside the gate body as the
/// best-effort STATIC proxy for "fails closed in the gate" — the runtime control-flow
/// dominance (this return is the None arm and precedes any delegation) is owned by the
/// `crates/verter_lsp/tests/owned_binding_gate.rs` tests, not fabricated here.
const OWNED_GATE_FAIL_CLOSED_RETURN: &str = "Ok(Vec::new())";

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

/// Byte span `[open_brace_inclusive, close_brace_exclusive)` of the method body for
/// `fn fn_name`, located by: (1) OPTIONALLY tracking entry into an impl block whose
/// signature line contains `impl_discriminant` (by that line + brace depth), (2) finding
/// the `fn_name` signature (WHILE inside that impl when a discriminant is required),
/// (3) scanning to its opening `{`, (4) brace-matching to the close. The brace scan runs
/// over [`blank_noncode`] so literal / comment braces never throw off the count. When
/// `impl_discriminant` is `None` the method is located file-wide (no impl gate). Returns
/// `None` when no such method body exists (a stale anchor). This is the shared body-span
/// locator behind BOTH the `configure_paths` allow-list scope and the OWNED-gate
/// binding-call scope.
fn method_body_span(
    src: &str,
    impl_discriminant: Option<&str>,
    fn_name: &str,
) -> Option<(usize, usize)> {
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
        if let Some(disc) = impl_discriminant {
            if trimmed.starts_with("impl") && line.contains(disc) {
                in_target_impl_depth = Some(brace_depth);
            }
        }

        // The first `fn_name` signature pins the body's opening brace (the `{` at or
        // after the signature in the code view). When an impl discriminant is required,
        // the signature only counts WHILE inside that impl; with no discriminant the
        // first file-wide `fn_name` signature wins.
        if found_sig_open.is_none()
            && (impl_discriminant.is_none() || in_target_impl_depth.is_some())
            && fn_name_introduced_on_line(line) == Some(fn_name)
        {
            // The signature may carry a multi-line parameter list / return type before
            // `{`; scan the whole remaining code view from the signature's line start.
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

/// Byte span of the [`ALLOWLISTED_INFERRED_CONFIG_FN`] method body on the
/// [`ALLOWLISTED_INFERRED_CONFIG_IMPL`] impl — the allow-list scope for the single
/// `compilerOptionsForInferredProjects` config call. A thin wrapper over the shared
/// [`method_body_span`] locator.
fn configure_paths_body_span(src: &str) -> Option<(usize, usize)> {
    method_body_span(
        src,
        Some(ALLOWLISTED_INFERRED_CONFIG_IMPL),
        ALLOWLISTED_INFERRED_CONFIG_FN,
    )
}

/// Byte span of the [`OWNED_GATE_FN`] method body on the [`OWNED_GATE_IMPL`] impl — the
/// REQ-1/REQ-6 brace-scope for the OWNED carrier-diagnostics gate. A thin wrapper over
/// the shared [`method_body_span`] locator. Pinning the binding calls to THIS span (not
/// merely "somewhere in composite.rs") is what stops a decoy call elsewhere in the file
/// from satisfying REQ-1/REQ-6.
fn composite_gate_body_span(src: &str) -> Option<(usize, usize)> {
    method_body_span(src, Some(OWNED_GATE_IMPL), OWNED_GATE_FN)
}

/// Does a LIVE CALL to `name` (`name(`, not the `fn name(` definition line, not a
/// comment) occur at a byte offset INSIDE `[span.0, span.1)` of `src`? The brace-scoped
/// counterpart of [`calls_named_fn`]: it applies the SAME call-vs-definition
/// discrimination, then additionally requires the call to fall inside the given body
/// span — so a decoy `name(` elsewhere in the file does NOT satisfy the check.
fn calls_named_fn_in_span(src: &str, name: &str, span: (usize, usize)) -> bool {
    let (open, close) = span;
    let def = format!("fn {name}(");
    let call = format!("{name}(");
    let mut line_start = 0usize;
    for line in src.split_inclusive('\n') {
        if !is_comment(line) && !line.contains(&def) {
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(&call) {
                let byte = line_start + search_from + rel;
                if byte >= open && byte < close {
                    return true;
                }
                search_from += rel + call.len();
            }
        }
        line_start += line.len();
    }
    false
}

/// Does a LIVE (non-comment) occurrence of the literal `needle` fall INSIDE
/// `[span.0, span.1)` of `src`? Reuses [`live_request_occurrences`]'s comment-skipping
/// byte-offset scan. Used for the fail-closed empty-result return proxy (a literal
/// expression, not a `name(` call shape).
fn live_needle_in_span(src: &str, needle: &str, span: (usize, usize)) -> bool {
    live_request_occurrences(src, needle)
        .into_iter()
        .any(|(byte, _)| byte >= span.0 && byte < span.1)
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

        // (S3 / REQ-6) No raw path-only OWNED bypass: the TEST-ONLY OWNED `--api` oracle
        // must NEVER be CALLED from PRODUCTION source. A production call to
        // `semantic_diagnostics_for_carrier_in_project` (the low-level per-tsconfig
        // per-query oracle) would be a raw path-only OWNED diagnostics route that bypasses
        // the always-present `BoundProject` admission layer. Its DEFINITION in `owned.rs`
        // (excluded here by the `fn NAME(` check in `calls_named_fn`) and its TEST call
        // sites (integration `tests/**` + filtered `*_tests.rs`, both outside this
        // production-`src` scan) are allowed; only a production `src` CALL is forbidden.
        if calls_named_fn(&content, "semantic_diagnostics_for_carrier_in_project") {
            violations.push(format!(
                "{rel}: PRODUCTION call to the test-only OWNED `--api` oracle \
                 `semantic_diagnostics_for_carrier_in_project` — a raw path-only OWNED \
                 diagnostics route that bypasses the always-present BoundProject admission layer \
                 (the oracle is DEFINED in owned.rs and TEST-called only)"
            ));
        }

        // (S4 / REQ-4) No `new_owned(...)` call OR definition passes a `tsconfig_path`
        // argument (belt-and-suspenders beside the resilient.rs whole-file `tsconfig_path`
        // ban below): a restart re-establishes the process only; the owning tsconfig is
        // taken PER QUERY, never threaded through the owned restart wrapper's arg list.
        if call_arglist_contains(&content, "new_owned", "tsconfig_path") {
            violations.push(format!(
                "{rel}: a `new_owned(...)` call/def passes a `tsconfig_path` argument — owned \
                 startup binds per project (the owning tsconfig is resolved per query); it must \
                 NOT thread a stored root `tsconfig_path` through the restart wrapper"
            ));
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

    // (S3 anchor) The PRODUCTION oracle-absence check (in the per-file loop above) is
    // anchored on a REAL symbol: the OWNED `--api` oracle DEFINITION must exist in
    // owned.rs. Without this anchor a renamed / removed oracle would make the absence
    // check vacuously clean (there would be nothing left to forbid a production call OF).
    if !owned_src
        .lines()
        .any(|line| line.contains("fn semantic_diagnostics_for_carrier_in_project("))
    {
        violations.push(
            "crates/verter_type_runtime/src/tsgo/owned.rs: the OWNED `--api` oracle \
             `semantic_diagnostics_for_carrier_in_project` DEFINITION is missing — the \
             production-call absence check is anchored on it and would be vacuous without it"
                .to_string(),
        );
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

    // ── REOPEN-A: per-project OWNED binding via the shared project-binding helper +
    //    BoundProject chain, replacing the root-tsconfig OWNED startup gate. ──
    let composite_rs = root.join("crates/verter_lsp/src/tsgo/composite.rs");
    let binding_rs = root.join("crates/verter_lsp/src/tsgo/project_binding.rs");
    let composite_src = fs::read_to_string(&composite_rs)
        .unwrap_or_else(|e| panic!("read {}: {e}", composite_rs.display()));
    let binding_src = fs::read_to_string(&binding_rs)
        .unwrap_or_else(|e| panic!("read {}: {e}", binding_rs.display()));

    // (1 / 6) The production OWNED carrier-diagnostics gate obtains a BoundProject from
    //     the shared project-binding helper BEFORE delegating, and the witness calls are
    //     BRACE-SCOPED to the gate method body: `resolve_carrier_bound(` (REQ-1) AND
    //     `into_bound(` (REQ-6) must occur INSIDE the `diagnostics_gated` method body on
    //     `impl TsgoCompositeProvider`, not merely somewhere in composite.rs — so a decoy
    //     call elsewhere in the file no longer satisfies REQ-1/REQ-6. Honest wording: the
    //     test-only `semantic_diagnostics_for_carrier_in_project` oracle is NOT a guard
    //     target — the witness is the gate's IN-BODY helper call + the helper's
    //     published-snapshot → resolver → ensure_project → BoundProject chain. This STATIC
    //     guard asserts the gate's source-level SHAPE; the RUNTIME fail-closed control-flow
    //     / dominance (a non-bound carrier yields NO external-TS diagnostics, resolved
    //     BEFORE delegation) is proven by the discriminating tests in
    //     `crates/verter_lsp/tests/owned_binding_gate.rs`
    //     (`every_non_bound_carrier_binding_variant_is_fail_closed_none`,
    //     `gate_no_project_carrier_fails_closed_to_empty`,
    //     `gate_non_bound_carrier_fails_closed_to_empty_background`,
    //     `gate_bound_carrier_delegates_to_owned`) — the static+runtime split is deliberate.
    match composite_gate_body_span(&composite_src) {
        None => violations.push(format!(
            "{OWNED_GATE_FILE}: could not locate the `{OWNED_GATE_FN}` OWNED carrier-diagnostics \
             gate method body on `impl {OWNED_GATE_IMPL}` — the REQ-1/REQ-6 brace-scope anchor is \
             stale (the gate was renamed, moved off the impl, or its braces no longer match); \
             re-pin it"
        )),
        Some(gate_span) => {
            // (1) REQ-1: obtain a BoundProject via a live `resolve_carrier_bound(` CALL
            //     INSIDE the gate body before delegating to TsgoOwnedProvider::get_diagnostics.
            if !calls_named_fn_in_span(&composite_src, "resolve_carrier_bound", gate_span) {
                violations.push(format!(
                    "{OWNED_GATE_FILE}: the `{OWNED_GATE_FN}` gate must obtain a BoundProject from \
                     the shared project-binding helper via a live `resolve_carrier_bound(` CALL \
                     INSIDE the gate method body (a decoy call elsewhere in composite.rs does NOT \
                     satisfy REQ-1) before delegating to TsgoOwnedProvider::get_diagnostics — no \
                     in-body live call found"
                ));
            }
            // (6) REQ-6: check the resolved BoundProject via a live `into_bound(` CALL INSIDE
            //     the gate body before delegating — no path-only OWNED open may bypass it.
            if !calls_named_fn_in_span(&composite_src, "into_bound", gate_span) {
                violations.push(format!(
                    "{OWNED_GATE_FILE}: the `{OWNED_GATE_FN}` gate must check the resolved \
                     BoundProject via a live `into_bound(` CALL INSIDE the gate method body (a \
                     decoy call elsewhere in composite.rs does NOT satisfy REQ-6) before \
                     delegating — no in-body live call found"
                ));
            }
            // (fail-closed shape) The non-bound path returns the empty external-TS result
            //     (`Ok(Vec::new())`) INSIDE the gate body. This asserts the fail-closed return
            //     EXISTS in the gate body — the best-effort static proxy for "fails closed"; the
            //     dominance property (it is the `into_bound()` None arm, preceding delegation) is
            //     owned by the runtime tests above, so the static guard does NOT fabricate a
            //     full-control-flow matcher.
            if !live_needle_in_span(&composite_src, OWNED_GATE_FAIL_CLOSED_RETURN, gate_span) {
                violations.push(format!(
                    "{OWNED_GATE_FILE}: the `{OWNED_GATE_FN}` gate body must contain the \
                     fail-closed `{OWNED_GATE_FAIL_CLOSED_RETURN}` no-external-TS-diagnostics \
                     return (the non-bound carrier path) — a non-bound carrier must yield NO \
                     external-TS diagnostics, never an OWNED `--lsp` fall-through"
                ));
            }
        }
    }
    // The carrier-companion classify CALL stays FILE-LEVEL: its gating role is the same
    // gate body, but the primary REQ-1/REQ-6 witnesses are the two brace-scoped calls
    // above. `carrier_source_of(` must be CALLED so a NON-carrier path is not gated and a
    // carrier companion is.
    if !calls_named_fn(&composite_src, "carrier_source_of") {
        violations.push(
            "crates/verter_lsp/src/tsgo/composite.rs: the gate must CALL `carrier_source_of(` to \
             classify a carrier companion so a NON-carrier path is not gated and a carrier \
             companion is — no live call found"
                .to_string(),
        );
    }
    for (needle, why) in [
        (
            "published_root",
            "the helper must resolve over the host's LIVE published snapshot",
        ),
        (
            "WorkspaceProjectResolver",
            "the helper must resolve ownership through the shared WorkspaceProjectResolver \
             (never a path-only inferred guess)",
        ),
        (
            "ensure_project",
            "the helper must mint the witness through the engine backend ensure_project",
        ),
        (
            "BoundProject",
            "the helper must yield the BoundProject witness the gate delegates behind",
        ),
    ] {
        if !has_live(&binding_src, needle) {
            violations.push(format!(
                "crates/verter_lsp/src/tsgo/project_binding.rs: the binding helper lost `{needle}` \
                 — {why}"
            ));
        }
    }

    // (3) The startup configured-project-presence SPAWN GATE exists — a live CALL to
    //     the bounded precondition (the per-project replacement for the root-only gate).
    //     Presence of the name alone (a comment, a `use`, the `verter_workspace`
    //     definition) is NOT enough: the spawn path must INVOKE it (`...anywhere(`).
    if !calls_named_fn(&main_src, "has_configured_ts_project_anywhere") {
        violations.push(
            "crates/verter_lsp/src/main.rs: owned tsgo startup must CALL the bounded \
             configured-project spawn precondition (a live `has_configured_ts_project_anywhere(` \
             call, not a bare mention) — the per-project replacement for the deleted root-only gate"
                .to_string(),
        );
    }

    // (4) ABSENCE: the root-only OWNED gate + the stored single tsconfig are DELETED.
    if has_live(&main_src, "require_owned_tsconfig") {
        violations.push(
            "crates/verter_lsp/src/main.rs: the root-only `require_owned_tsconfig` OWNED gate must \
             be ABSENT (deleted, not merely uncalled) — replaced by per-project binding"
                .to_string(),
        );
    }
    if has_live(&main_src, "join(\"tsconfig.json\")") {
        violations.push(
            "crates/verter_lsp/src/main.rs: a root-only `workspace_root.join(\"tsconfig.json\")` \
             OWNED startup gate must be ABSENT — owned tsgo binds per project, not root-only"
                .to_string(),
        );
    }
    if has_live(&owned_src, "tsconfig_path") {
        violations.push(
            "crates/verter_type_runtime/src/tsgo/owned.rs: the owned provider must store NO single \
             `tsconfig_path` — the `--api` oracle takes the owning tsconfig PER QUERY"
                .to_string(),
        );
    }
    if has_live(&resilient_src, "tsconfig_path") {
        violations.push(
            "crates/verter_lsp/src/tsgo/resilient.rs: the owned restart backend must store NO \
             `tsconfig_path`, and `new_owned(...)` must take no `tsconfig_path` param — a restart \
             re-establishes the process only"
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

/// Does a non-comment line of `src` CALL `name` (`name(`), as opposed to merely
/// DEFINING it (`fn name(`) or naming it in prose? Generalises the CALL-vs-definition
/// rigor of the retired `calls_require_owned_tsconfig` helper: a live CALL has the
/// open-paren call shape on a non-comment line and is not the `fn name(` definition line.
/// Mere presence of the name (a comment, a `use`, a doc link) is NOT a call.
fn calls_named_fn(src: &str, name: &str) -> bool {
    let def = format!("fn {name}(");
    let call = format!("{name}(");
    src.lines()
        .any(|line| !is_comment(line) && !line.contains(&def) && line.contains(&call))
}

/// Does ANY `callee(` argument list in `src` contain the identifier `arg`? Paren-matches
/// each `callee(` occurrence over a [`blank_noncode`] view (so literal parens inside
/// strings / comments never miscount) and searches the blanked CODE-only view for `arg`
/// (so an `arg` substring inside a string literal is not a false match). Handles
/// multi-line signatures AND multi-line calls. A `callee(` occurrence is counted only at
/// an identifier boundary, so a longer name like `wrap_new_owned(` does not match
/// `new_owned(`.
fn call_arglist_contains(src: &str, callee: &str, arg: &str) -> bool {
    let code = blank_noncode(src);
    let bytes = code.as_bytes();
    let needle = format!("{callee}(");
    let mut from = 0usize;
    while let Some(rel) = code[from..].find(&needle) {
        let start = from + rel;
        // `open` is the byte index of the call's opening paren.
        let open = start + needle.len() - 1;
        let at_boundary =
            start == 0 || !(bytes[start - 1].is_ascii_alphanumeric() || bytes[start - 1] == b'_');
        if !at_boundary {
            from = start + needle.len();
            continue;
        }
        // Paren-match from `open` to its close over the code view.
        let mut depth = 0usize;
        let mut idx = open;
        let mut close = None;
        while idx < bytes.len() {
            match bytes[idx] {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(idx);
                        break;
                    }
                }
                _ => {}
            }
            idx += 1;
        }
        match close {
            Some(close) => {
                if code[open + 1..close].contains(arg) {
                    return true;
                }
                from = close + 1;
            }
            None => break,
        }
    }
    false
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

    // ── REOPEN-A per-project OWNED binding predicates: each FIRES on a synthetic
    //    reintroduction and is CLEAN on the real per-project-binding shape. The
    //    CALL-shape predicates additionally stay CLEAN on the bare DEFINITION line. ──

    // (S2 / point 1) the helper-CALL gate witness: a live `resolve_carrier_bound(` call
    // reads as CALLED; the bare DEFINITION line and a comment do NOT.
    assert!(
        calls_named_fn(
            "    let carrier = resolve_carrier_bound(&self.host, &source).into_bound();",
            "resolve_carrier_bound"
        ),
        "a live resolve_carrier_bound( gate call must read as CALLED"
    );
    assert!(
        !calls_named_fn(
            "pub fn resolve_carrier_bound(host: &VerterHost, source: &str) -> CarrierBinding {",
            "resolve_carrier_bound"
        ),
        "the resolve_carrier_bound DEFINITION alone must read as NOT CALLED"
    );
    assert!(
        !calls_named_fn(
            "    // resolve_carrier_bound is the shared binding helper",
            "resolve_carrier_bound"
        ),
        "a comment naming the helper is not a live gate call"
    );

    // (S2 / point 1) the carrier-companion classify CALL, def-excluded.
    assert!(
        calls_named_fn(
            "        let Some(source) = carrier_source_of(path) else {",
            "carrier_source_of"
        ),
        "a live carrier_source_of( classify call must read as CALLED"
    );
    assert!(
        !calls_named_fn(
            "fn carrier_source_of(provider_path: &str) -> Option<String> {",
            "carrier_source_of"
        ),
        "the carrier_source_of DEFINITION alone must read as NOT CALLED"
    );

    // (S2 / point 6) the BoundProject witness check before delegation, as a live call.
    assert!(
        calls_named_fn(
            "    let Some(carrier) = c.into_bound() else {",
            "into_bound"
        ),
        "a live into_bound( witness check must read as CALLED"
    );
    assert!(
        !calls_named_fn(
            "    pub fn into_bound(self) -> Option<BoundCarrier> {",
            "into_bound"
        ),
        "the into_bound DEFINITION alone must read as NOT CALLED"
    );

    // ── REQ-1 / REQ-6 BRACE-SCOPED to the OWNED gate body: the binding calls +
    //    fail-closed return must live INSIDE the `diagnostics_gated` method on
    //    `impl TsgoCompositeProvider`. A decoy of each placed in a SIBLING method OUTSIDE
    //    the gate body must NOT satisfy the brace-scoped predicate (where the retired
    //    file-level `calls_named_fn` would be fooled). Mirrors the configure_paths
    //    body-span self-tests: async + multi-line signature + nested/literal braces. ──

    // Wrap `methods` in the inherent `impl TsgoCompositeProvider` so
    // `composite_gate_body_span` has the real anchor shape.
    let gate_wrap =
        |methods: &str| -> String { format!("impl TsgoCompositeProvider {{\n{methods}}}\n") };

    // The REAL gate shape (async, multi-line signature, nested `else { … }` blocks): the
    // in-body `resolve_carrier_bound(` / `into_bound` / `Ok(Vec::new())` all fall INSIDE
    // the brace-matched gate span.
    let real_gate = gate_wrap(
        "    async fn diagnostics_gated(\n        &self,\n        path: &str,\n        \
         background: bool,\n    ) -> Result<Vec<TypeDiagnostic>, TypeProviderError> {\n        \
         let Some(source) = carrier_source_of(path) else {\n            \
         return self.owned_diagnostics(path, background).await;\n        };\n        \
         let Some(carrier) =\n            \
         project_binding::resolve_carrier_bound(&self.host, &source).into_bound()\n        \
         else {\n            return Ok(Vec::new());\n        };\n        \
         let owned = self.owned_diagnostics(path, background).await?;\n        Ok(owned)\n    }\n",
    );
    let real_span = composite_gate_body_span(&real_gate).expect("real gate body span");
    assert!(
        calls_named_fn_in_span(&real_gate, "resolve_carrier_bound", real_span),
        "the real IN-BODY resolve_carrier_bound( call must satisfy the brace-scoped REQ-1 predicate"
    );
    assert!(
        calls_named_fn_in_span(&real_gate, "into_bound", real_span),
        "the real IN-BODY into_bound( call must satisfy the brace-scoped REQ-6 predicate"
    );
    assert!(
        live_needle_in_span(&real_gate, "Ok(Vec::new())", real_span),
        "the real IN-BODY fail-closed Ok(Vec::new()) return must satisfy the brace-scoped proxy"
    );

    // A DECOY of each token placed in a SIBLING method AFTER the gate close: the gate
    // body itself just delegates (no binding call), so the brace-scoped predicates are
    // NOT satisfied — the whole point of the strengthening. The retired file-level
    // `calls_named_fn` IS fooled by the same decoy (asserted for contrast).
    let decoy_gate = gate_wrap(
        "    async fn diagnostics_gated(\n        &self,\n        path: &str,\n        \
         background: bool,\n    ) -> Result<Vec<TypeDiagnostic>, TypeProviderError> {\n        \
         self.owned_diagnostics(path, background).await\n    }\n\n    \
         async fn decoy(&self, p: &str) -> Result<Vec<TypeDiagnostic>, TypeProviderError> {\n        \
         let _c = project_binding::resolve_carrier_bound(&self.host, p).into_bound();\n        \
         Ok(Vec::new())\n    }\n",
    );
    let decoy_span = composite_gate_body_span(&decoy_gate).expect("decoy gate body span");
    assert!(
        !calls_named_fn_in_span(&decoy_gate, "resolve_carrier_bound", decoy_span),
        "a decoy resolve_carrier_bound( OUTSIDE the gate body must NOT satisfy the brace-scoped \
         REQ-1 predicate"
    );
    assert!(
        !calls_named_fn_in_span(&decoy_gate, "into_bound", decoy_span),
        "a decoy into_bound( OUTSIDE the gate body must NOT satisfy the brace-scoped REQ-6 predicate"
    );
    assert!(
        !live_needle_in_span(&decoy_gate, "Ok(Vec::new())", decoy_span),
        "a decoy Ok(Vec::new()) OUTSIDE the gate body must NOT satisfy the brace-scoped fail-closed \
         proxy"
    );
    assert!(
        calls_named_fn(&decoy_gate, "resolve_carrier_bound")
            && calls_named_fn(&decoy_gate, "into_bound"),
        "contrast: the retired FILE-LEVEL calls_named_fn IS fooled by the sibling decoy — the \
         brace-scope is exactly what closes that gap"
    );

    // The gate-body-span locator brace-matches over nested code braces AND a literal-brace
    // string (mirrors the real body's `else { … }` blocks + a `json!({ "}" })`): the span
    // ends at the method's TRUE closing brace, not a literal `}`, so an in-body needle
    // after the literal brace is still detected.
    let nested_gate = gate_wrap(
        "    async fn diagnostics_gated(\n        &self,\n        path: &str,\n    ) -> \
         Result<Vec<TypeDiagnostic>, TypeProviderError> {\n        \
         let _ = json!({ \"k\": \"}\" });\n        \
         let Some(c) = resolve_carrier_bound(h, path).into_bound() else {\n            \
         return Ok(Vec::new());\n        };\n        Ok(c.diags())\n    }\n",
    );
    let nested_span = composite_gate_body_span(&nested_gate).expect("nested gate body span");
    assert!(
        calls_named_fn_in_span(&nested_gate, "into_bound", nested_span)
            && live_needle_in_span(&nested_gate, "Ok(Vec::new())", nested_span),
        "an into_bound( call / fail-closed return AFTER a literal-brace string must fall within the \
         matched gate span (blank_noncode must not let a string `}}` close it early)"
    );

    // A STALE ANCHOR: no `diagnostics_gated` on the impl at all (renamed / moved off)
    // makes the gate span unlocatable — the main test reports the stale anchor.
    assert!(
        composite_gate_body_span("impl TsgoCompositeProvider {\n    async fn other(&self) {}\n}\n")
            .is_none(),
        "a missing diagnostics_gated body must make the composite gate span unlocatable"
    );

    // The ACTUAL composite.rs exposes a real, non-empty gate span carrying all three
    // in-body witnesses — proves the brace-scope pin is anchored on the real method, not
    // vacuously absent (the strengthened main-test predicate is non-vacuous on the tree).
    let live_composite = fs::read_to_string(workspace_root().join(OWNED_GATE_FILE))
        .expect("read the live composite.rs");
    let live_gate_span = composite_gate_body_span(&live_composite)
        .expect("the live composite.rs must expose a real diagnostics_gated body span");
    assert!(
        live_gate_span.1 > live_gate_span.0,
        "the live gate span must be non-empty"
    );
    assert!(
        calls_named_fn_in_span(&live_composite, "resolve_carrier_bound", live_gate_span),
        "the live gate body must carry the in-body resolve_carrier_bound( REQ-1 witness"
    );
    assert!(
        calls_named_fn_in_span(&live_composite, "into_bound", live_gate_span),
        "the live gate body must carry the in-body into_bound( REQ-6 witness"
    );
    assert!(
        live_needle_in_span(
            &live_composite,
            OWNED_GATE_FAIL_CLOSED_RETURN,
            live_gate_span
        ),
        "the live gate body must carry the in-body fail-closed Ok(Vec::new()) return"
    );

    // (S1 / point 3) the startup spawn-gate CALL: a live call reads CALLED; the
    // `verter_workspace` definition and a comment mention do NOT.
    assert!(
        calls_named_fn(
            "    if !verter_workspace::config::has_configured_ts_project_anywhere(root) { \
             return Err(reason); }",
            "has_configured_ts_project_anywhere"
        ),
        "a live has_configured_ts_project_anywhere( spawn-gate call must read as CALLED"
    );
    assert!(
        !calls_named_fn(
            "pub fn has_configured_ts_project_anywhere(root: &Path) -> bool {",
            "has_configured_ts_project_anywhere"
        ),
        "the has_configured_ts_project_anywhere DEFINITION alone must read as NOT CALLED"
    );
    assert!(
        !calls_named_fn(
            "    // has_configured_ts_project_anywhere is the bounded spawn precondition",
            "has_configured_ts_project_anywhere"
        ),
        "a comment naming the spawn precondition must read as NOT CALLED"
    );

    // (S3 / REQ-6) the PRODUCTION oracle-absence predicate: a synthetic production CALL to
    // the test-only OWNED `--api` oracle trips it; the DEFINITION and a comment do NOT
    // (this is exactly the `calls_named_fn` verdict the per-file production scan applies).
    assert!(
        calls_named_fn(
            "        let d = self.owned.semantic_diagnostics_for_carrier_in_project(&p, &cfg).await;",
            "semantic_diagnostics_for_carrier_in_project"
        ),
        "a synthetic PRODUCTION call to the OWNED --api oracle must trip the absence check"
    );
    assert!(
        !calls_named_fn(
            "    pub async fn semantic_diagnostics_for_carrier_in_project(",
            "semantic_diagnostics_for_carrier_in_project"
        ),
        "the oracle DEFINITION alone must NOT trip the production-call absence check"
    );
    assert!(
        !calls_named_fn(
            "    //! reflection ORACLE (`semantic_diagnostics_for_carrier_in_project`). It ...",
            "semantic_diagnostics_for_carrier_in_project"
        ),
        "a comment naming the oracle must NOT trip the production-call absence check"
    );

    // (S4 / REQ-4) the `new_owned(` arg-list check: a `tsconfig_path` argument (single OR
    // multi-line) trips it; an arg list without one is CLEAN; a longer callee that merely
    // ends in `new_owned` does not match at a non-identifier boundary.
    assert!(
        call_arglist_contains(
            "let r = tsgo_resilient::new_owned(owned, crash, bin, root, tsconfig_path);",
            "new_owned",
            "tsconfig_path"
        ),
        "a new_owned( call passing tsconfig_path must trip the param check"
    );
    assert!(
        call_arglist_contains(
            "pub fn new_owned(\n    provider: P,\n    tsconfig_path: String,\n    max: u32,\n) {",
            "new_owned",
            "tsconfig_path"
        ),
        "a MULTI-LINE new_owned definition with a tsconfig_path param must trip the param check"
    );
    assert!(
        !call_arglist_contains(
            "let r = tsgo_resilient::new_owned(owned, crash_notify, tsgo_bin, root_uri, client, 3);",
            "new_owned",
            "tsconfig_path"
        ),
        "a new_owned( call WITHOUT tsconfig_path must be CLEAN"
    );
    assert!(
        !call_arglist_contains(
            "let r = wrap_new_owned(owned, tsconfig_path);",
            "new_owned",
            "tsconfig_path"
        ),
        "a longer callee ending in new_owned must NOT match at a non-identifier boundary"
    );

    // (4) ABSENCE predicates FIRE on a synthetic reintroduction, CLEAN otherwise.
    assert!(
        has_live(
            "    let cfg = require_owned_tsconfig(root)?;",
            "require_owned_tsconfig"
        ),
        "a reintroduced require_owned_tsconfig call must trip the absence check"
    );
    assert!(
        !has_live(
            "    // require_owned_tsconfig was replaced by per-project binding",
            "require_owned_tsconfig"
        ),
        "a comment naming the deleted helper must NOT trip the absence check"
    );
    assert!(
        has_live(
            "    let ts = workspace_root.join(\"tsconfig.json\");",
            "join(\"tsconfig.json\")"
        ),
        "a reintroduced root-only join(\"tsconfig.json\") gate must trip the absence check"
    );
    assert!(
        has_live("    tsconfig_path: String,", "tsconfig_path"),
        "a reintroduced stored tsconfig_path field must trip the absence check"
    );
    assert!(
        !has_live("        tsconfig: &str,", "tsconfig_path"),
        "the per-query `tsconfig` param must NOT trip the `tsconfig_path` absence check"
    );
    // (1/6) the binding-helper chain tokens FIRE on a synthetic helper, CLEAN on a stub.
    for needle in [
        "published_root",
        "WorkspaceProjectResolver",
        "ensure_project",
        "BoundProject",
    ] {
        assert!(
            has_live(
                "    let r = WorkspaceProjectResolver::new(published_root, ..); \
                 backend.ensure_project(b) -> BoundProject",
                needle
            ),
            "the binding-helper chain token `{needle}` must read as present in the helper shape"
        );
    }

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

// ── STRENGTHENING (SB6c-5): carrier FEATURE queries are admission-gated too ──
//
// SB6c-5 lands OWNED carrier FEATURE admission: every carrier feature method on
// `TsgoCompositeProvider` gates on a resolved `BoundProject` (memoized by the
// generation-scoped `CarrierAdmissionCache`) through the `feature_admits` helper BEFORE
// delegating to OWNED, exactly like the always-present carrier-diagnostics gate. The
// STRENGTHENING tail test below is ADDITIVE — it does NOT touch the main test @654 or its
// REQ-1..N diagnostics-gate assertions — and pins FIVE properties:
//   1. EXHAUSTIVENESS (two complementary layers): (a) the `ProviderFeature` variant set
//      EQUALS the gated-method registry, and every REGISTERED gated method routes through a
//      live in-body `feature_admits(` call naming its `ProviderFeature::<Variant>` — a new
//      registry entry without a gate (or a variant without a method) breaks the tie; AND
//      (b) the DELEGATION PARTITION walks EVERY `self.owned.*` call in `impl TypeProvider
//      for TsgoCompositeProvider` (not just the registry rows) and requires each enclosing
//      method to be a gated feature whose delegation is DOMINATED by a `feature_admits(`
//      gate OR an explicitly-allowlisted lifecycle/passthrough method — so a NEW ungated
//      feature method (the `get_code_actions` class, and any future one) is caught even if
//      it never enters the registry, and a gated method whose gate does not precede the
//      OWNED call is caught too. Layer (b) is the true-exhaustiveness backstop that the
//      earlier registry-only check lacked (it closes the false-exhaustiveness gap a
//      registry-only check leaves open).
//   2. Denied carrier admission never delegates to OWNED `--lsp`: the STATIC shape is the
//      in-body gate call (delegation is conditional on admission); the RUNTIME dominance (a
//      denied carrier's OWNED counter stays 0, never a `--lsp` fall-through) is owned by the
//      discriminating tests in `crates/verter_lsp/tests/owned_binding_gate.rs`
//      (`feature_external_only_denied_carrier_serves_empty_no_owned_call`,
//      `feature_mixed_read_denied_carrier_serves_external_default_no_owned_call`,
//      `feature_completion_denied_carrier_serves_native_only_no_owned_call`,
//      `feature_rename_denied_carrier_serves_native_only_no_owned_call`) — the same
//      static+runtime split the diagnostics gate uses.
//   3. Plain `.ts`/`.tsx` are NOT gated: `feature_admits` classifies the carrier companion
//      via `carrier_source_of` and leaves a non-carrier ungated.
//   4. Feature admission rides the ONE shared binding source: the `CarrierAdmissionCache`
//      resolves through `resolve_carrier_bound(` (the published_root → WorkspaceProjectResolver
//      → ProjectBinding → ensure_project → BoundProject chain in project_binding.rs), never a
//      feature-local fork.
//   5. The `--api` oracle stays test-only: the feature-bearing composite.rs never calls
//      `semantic_diagnostics_for_carrier_in_project` (the main test bans it across ALL
//      production src; this pins the feature file explicitly).

/// The gated carrier FEATURE methods on `TsgoCompositeProvider`, each paired with the
/// `ProviderFeature` variant it names. The guard's canonical registry — asserted EQUAL (as
/// a set) to the `ProviderFeature` enum variants declared in composite.rs, so a new variant
/// without a gated method (or a gated method without a variant) breaks the guard.
const GATED_FEATURE_METHODS: &[(&str, &str)] = &[
    ("get_type_definition", "TypeDefinition"),
    ("get_signature_help", "SignatureHelp"),
    ("get_semantic_tokens", "SemanticTokens"),
    ("get_hover", "Hover"),
    ("get_definition", "Definition"),
    ("get_references", "References"),
    ("get_document_highlights", "DocumentHighlights"),
    ("get_inlay_hints", "InlayHints"),
    ("get_completions", "Completions"),
    ("get_completion_details", "CompletionDetails"),
    ("resolve_completion", "ResolveCompletion"),
    ("get_rename_locations", "RenameLocations"),
    ("get_code_actions", "CodeActions"),
];

/// The NON-feature `TypeProvider` methods on `impl TypeProvider for TsgoCompositeProvider`
/// that legitimately delegate RAW to `self.owned` (no `feature_admits` gate): carrier
/// lifecycle, config/workspace, and pure passthrough. The delegation partition
/// (`owned_delegation_partition_violations`) requires EVERY `self.owned.*` call to be
/// EITHER a gated feature dominated by a `feature_admits(` gate OR an enclosing method
/// named here — so a NEW ungated feature method (a `--lsp` self-discovery fall-through)
/// that is neither is caught even if it never enters the `ProviderFeature` registry. This
/// allowlist is MAINTAINED: adding a genuine lifecycle/passthrough delegation requires a
/// row here (the guard cannot infer feature-vs-lifecycle intent structurally). Add a
/// FEATURE method to `GATED_FEATURE_METHODS` + a gate instead; only list a method here
/// when it is genuinely non-feature lifecycle/passthrough. `get_diagnostics` /
/// `_background` route through `diagnostics_gated` (no direct in-body `self.owned`), so
/// they carry no raw delegation to partition; they are listed for completeness.
const LIFECYCLE_PASSTHROUGH_METHODS: &[&str] = &[
    "provider_id",
    "supports_completion_resolve",
    "open_file",
    "load_file",
    "update_file",
    "close_file",
    "open_file_background",
    "load_file_background",
    "update_file_background",
    "close_file_background",
    "open_file_normal",
    "load_file_normal",
    "update_file_normal",
    "close_file_normal",
    "configure_paths",
    "configure_paths_background",
    "notify_carrier_changed",
    "register_carrier_member",
    "resync_open_files",
    "update_workspace_folders",
    "update_workspace_folders_background",
    "child_pid",
    "shutdown",
    "get_diagnostics",
    "get_diagnostics_background",
];

/// The trait-impl discriminant scoping the gated feature methods (the trait impl, distinct
/// from the inherent `impl TsgoCompositeProvider` that carries `diagnostics_gated` +
/// `feature_admits`).
const COMPOSITE_FEATURE_IMPL: &str = "TypeProvider for TsgoCompositeProvider";
/// The admission helper every gated feature method must CALL in its body before OWNED.
const FEATURE_ADMIT_FN: &str = "feature_admits";
/// The exhaustive gated-feature registry enum in composite.rs.
const PROVIDER_FEATURE_ENUM: &str = "ProviderFeature";
/// The shared binding-source file the feature admission cache lives in / resolves through.
const PROJECT_BINDING_FILE: &str = "crates/verter_lsp/src/tsgo/project_binding.rs";

/// The fieldless variant identifiers declared in `enum <enum_name> { ... }` of `src`,
/// skipping doc/comment lines. Brace-matches the enum body over a [`blank_noncode`] view
/// (offset-preserving) so a literal brace never miscounts, then pulls the leading
/// PascalCase identifier of each non-comment body line. Returns an empty vec when no such
/// enum exists.
fn enum_variants(src: &str, enum_name: &str) -> Vec<String> {
    let code = blank_noncode(src);
    let decl = format!("enum {enum_name} ");
    let Some(decl_pos) = code.find(&decl) else {
        return Vec::new();
    };
    let Some(rel_open) = code[decl_pos..].find('{') else {
        return Vec::new();
    };
    let open = decl_pos + rel_open;
    let bytes = code.as_bytes();
    let mut depth = 0usize;
    let mut idx = open;
    let mut close = None;
    while idx < bytes.len() {
        match bytes[idx] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    close = Some(idx);
                    break;
                }
            }
            _ => {}
        }
        idx += 1;
    }
    let Some(close) = close else {
        return Vec::new();
    };
    // The blanked view is offset-preserving, so [open+1, close) maps 1:1 onto `src`.
    let mut variants = Vec::new();
    for line in src[open + 1..close].lines() {
        if is_comment(line) {
            continue;
        }
        let ident: String = line
            .trim()
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if ident.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
            variants.push(ident);
        }
    }
    variants
}

/// Byte span `[open_brace_inclusive, close_brace_exclusive]` of the FIRST `impl` block
/// whose signature line contains `disc`, located by brace-matching over a [`blank_noncode`]
/// view (so literal / comment braces never miscount). `None` when no such impl exists.
fn impl_block_span(src: &str, disc: &str) -> Option<(usize, usize)> {
    let code = blank_noncode(src);
    let code_bytes = code.as_bytes();
    let mut line_start = 0usize;
    let mut open = None;
    for line in src.split_inclusive('\n') {
        if line.trim_start().starts_with("impl") && line.contains(disc) {
            if let Some(rel) = code[line_start..].find('{') {
                open = Some(line_start + rel);
                break;
            }
        }
        line_start += line.len();
    }
    let open = open?;
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

/// EVERY method in the impl block identified by `disc`, as `(fn_name, (body_open,
/// body_close))`. Each body span is the brace-matched `{ ... }` after the signature,
/// located over a [`blank_noncode`] view. Signature lines OUTSIDE the impl body are
/// ignored, so a sibling impl's methods are never attributed here. Returns an empty vec
/// when the impl block is absent (a stale anchor). This is the enumerator the delegation
/// partition walks — it does NOT depend on a hardcoded method list, so a NEW method is
/// enumerated and classified automatically.
fn impl_method_spans(src: &str, disc: &str) -> Vec<(String, (usize, usize))> {
    let Some((impl_open, impl_close)) = impl_block_span(src, disc) else {
        return Vec::new();
    };
    let code = blank_noncode(src);
    let code_bytes = code.as_bytes();
    let mut methods = Vec::new();
    let mut line_start = 0usize;
    for line in src.split_inclusive('\n') {
        let this = line_start;
        line_start += line.len();
        // Only signature lines strictly INSIDE the impl body (after its opening brace,
        // before its close). The impl's own signature line sits at/ before `impl_open`.
        if this <= impl_open || this >= impl_close {
            continue;
        }
        let Some(name) = fn_name_introduced_on_line(line) else {
            continue;
        };
        // The body opening brace is the first `{` at/after this signature line in the
        // code view (params / return types carry no braces in these signatures).
        let Some(rel) = code[this..impl_close].find('{') else {
            continue;
        };
        let open = this + rel;
        let mut depth = 0usize;
        let mut idx = open;
        let mut close = None;
        while idx < code_bytes.len() {
            match code_bytes[idx] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(idx);
                        break;
                    }
                }
                _ => {}
            }
            idx += 1;
        }
        if let Some(close) = close {
            methods.push((name.to_string(), (open, close)));
        }
    }
    methods
}

/// Every `self.owned[ws].<method>(` delegation whose `self.owned` token falls inside
/// `[span]` of the ALREADY-BLANKED `code`, as `(delegated_method, self_owned_byte)`.
/// Tolerates the multi-line `self.owned\n    .method(` receiver split. Runs over the
/// blanked view so a `self.owned` inside a string / comment never matches, and a
/// `self.owned_diagnostics` field access (no `.` after `owned`) is NOT a delegation.
fn owned_delegations_in_span(code: &str, span: (usize, usize)) -> Vec<(String, usize)> {
    let (open, close) = span;
    let bytes = code.as_bytes();
    let needle = "self.owned";
    let mut out = Vec::new();
    let mut from = open;
    while let Some(rel) = code[from..close].find(needle) {
        let at = from + rel;
        let mut i = at + needle.len();
        while i < close && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < close && bytes[i] == b'.' {
            i += 1;
            while i < close && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            let name_start = i;
            while i < close && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            if i > name_start {
                out.push((code[name_start..i].to_string(), at));
            }
        }
        from = at + needle.len();
    }
    out
}

/// The byte offset of the FIRST live `name(` call inside `[span]` of `src`, skipping the
/// `fn name(` definition form and comment lines. `None` when absent. The offset-returning
/// counterpart of [`calls_named_fn_in_span`], used to prove a `feature_admits(` gate
/// DOMINATES (precedes) the `self.owned` delegation in the same method body.
fn first_call_offset_in_span(src: &str, name: &str, span: (usize, usize)) -> Option<usize> {
    let (open, close) = span;
    let def = format!("fn {name}(");
    let call = format!("{name}(");
    let mut line_start = 0usize;
    for line in src.split_inclusive('\n') {
        if !is_comment(line) && !line.contains(&def) {
            let mut search_from = 0usize;
            while let Some(rel) = line[search_from..].find(&call) {
                let byte = line_start + search_from + rel;
                if byte >= open && byte < close {
                    return Some(byte);
                }
                search_from += rel + call.len();
            }
        }
        line_start += line.len();
    }
    None
}

/// The net brace depth at `to`, counted from `from` over the offset-preserving blanked
/// `code` (so string/comment braces never miscount). Used to require a gated method's
/// `self.owned` delegation to sit INSIDE the `feature_admits`-guarded block — a strictly
/// deeper depth than the gate call itself — not merely lexically after it. `from`/`to` are
/// absolute offsets into the blanked view (which shares `src`'s offsets); `from <= to`.
fn brace_depth_at(code: &str, from: usize, to: usize) -> i32 {
    code[from..to].bytes().fold(0i32, |d, b| match b {
        b'{' => d + 1,
        b'}' => d - 1,
        _ => d,
    })
}

/// The EXHAUSTIVE `self.owned.*` delegation partition for `impl TypeProvider for
/// TsgoCompositeProvider`: enumerate EVERY method of that impl, and for every method that
/// delegates to `self.owned`, require it to be EITHER
///   (a) a gated FEATURE method (in [`GATED_FEATURE_METHODS`]) whose `self.owned`
///       delegation is DOMINATED by a `feature_admits(` gate earlier in the same body, OR
///   (b) an explicitly [`LIFECYCLE_PASSTHROUGH_METHODS`]-allowlisted non-feature method.
/// Any `self.owned` delegation in a method that is NEITHER is a violation — so a NEW
/// ungated feature method (the exact `get_code_actions` class, and any future one) is
/// caught even if it never enters the registry, and a gated method whose gate does NOT
/// precede the delegation is caught too. Returns one string per violation. A missing impl
/// block (stale anchor) is itself a violation (the partition would otherwise be vacuous).
fn owned_delegation_partition_violations(composite_src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let methods = impl_method_spans(composite_src, COMPOSITE_FEATURE_IMPL);
    if methods.is_empty() {
        out.push(format!(
            "{OWNED_GATE_FILE}: could not enumerate any method on `impl {COMPOSITE_FEATURE_IMPL}` \
             — the delegation-partition anchor is stale (the impl was renamed or its braces no \
             longer match); the partition would be vacuous"
        ));
        return out;
    }
    let code = blank_noncode(composite_src);
    let gated: BTreeSet<&str> = GATED_FEATURE_METHODS.iter().map(|(m, _)| *m).collect();
    let allow: BTreeSet<&str> = LIFECYCLE_PASSTHROUGH_METHODS.iter().copied().collect();

    for (method, span) in &methods {
        let delegations = owned_delegations_in_span(&code, *span);
        let Some(first_deleg) = delegations.iter().map(|(_, at)| *at).min() else {
            // No raw `self.owned` delegation in this method (e.g. get_diagnostics routes
            // through diagnostics_gated) — nothing to partition.
            continue;
        };
        if gated.contains(method.as_str()) {
            // (a) DOMINANCE: a `feature_admits(` gate must PRECEDE the delegation AND the
            // delegation must sit INSIDE a deeper brace block opened after that gate (the
            // `if feature_admits(..) { .. self.owned.. }` guarded body). Bare token-order
            // (`admit` before `owned`) is NOT enough: an empty `if feature_admits(..) {}`
            // followed by an ungated `self.owned` call satisfies mere order yet delegates
            // ungated. Requiring the OWNED call at a STRICTLY DEEPER brace depth than the
            // gate call rejects that. (Source-text depth approximates control-flow dominance
            // for this impl's shape; the load-bearing exhaustiveness — a fully-ungated
            // feature method — is caught by the (b) partition regardless.)
            match first_call_offset_in_span(composite_src, FEATURE_ADMIT_FN, *span) {
                Some(admit_at)
                    if admit_at < first_deleg
                        && brace_depth_at(&code, span.0, first_deleg)
                            > brace_depth_at(&code, span.0, admit_at) => {}
                _ => out.push(format!(
                    "{OWNED_GATE_FILE}: gated feature method `{method}` delegates to `self.owned` \
                     WITHOUT a dominating `{FEATURE_ADMIT_FN}(` gate GUARDING the delegation — the \
                     OWNED call must sit INSIDE the `if {FEATURE_ADMIT_FN}(..) {{ .. }}` block \
                     (a strictly deeper brace depth than the gate), not after / outside / absent \
                     (an ungated `--lsp` self-discovery fall-through)"
                )),
            }
        } else if !allow.contains(method.as_str()) {
            // (b catch-all) Neither a gated feature nor an allowlisted lifecycle method.
            out.push(format!(
                "{OWNED_GATE_FILE}: method `{method}` delegates to `self.owned` but is NEITHER a \
                 gated `ProviderFeature` (dominated by `{FEATURE_ADMIT_FN}`) NOR an allowlisted \
                 lifecycle/passthrough method — a new ungated feature is a `--lsp` self-discovery \
                 fall-through; add it to GATED_FEATURE_METHODS + a `feature_admits` gate + a \
                 `ProviderFeature` variant, or (only if genuinely non-feature) to \
                 LIFECYCLE_PASSTHROUGH_METHODS"
            ));
        }
    }
    out
}

/// SB6c-5 STRENGTHENING: carrier FEATURE queries ride the SAME `BoundProject` admission as
/// diagnostics — every feature method gates through `feature_admits` before OWNED, the gate
/// is exhaustively tied to the `ProviderFeature` registry, EVERY `self.owned.*` delegation
/// in the trait impl is partitioned into a dominated gated feature or an allowlisted
/// lifecycle method (so a NEW ungated feature is caught, not just the registry ones), a
/// non-carrier is ungated, and the admission rides the ONE shared binding source. Additive
/// to the main test.
#[test]
fn carrier_features_are_admission_gated() {
    let root = workspace_root();
    let composite_src = fs::read_to_string(root.join(OWNED_GATE_FILE))
        .unwrap_or_else(|e| panic!("read {OWNED_GATE_FILE}: {e}"));
    let binding_src = fs::read_to_string(root.join(PROJECT_BINDING_FILE))
        .unwrap_or_else(|e| panic!("read {PROJECT_BINDING_FILE}: {e}"));

    let mut violations: Vec<String> = Vec::new();

    // (1) EXHAUSTIVENESS: the declared ProviderFeature variant set EQUALS the gated-method
    //     registry's variant set.
    let declared: BTreeSet<String> = enum_variants(&composite_src, PROVIDER_FEATURE_ENUM)
        .into_iter()
        .collect();
    let registry: BTreeSet<String> = GATED_FEATURE_METHODS
        .iter()
        .map(|(_, v)| (*v).to_string())
        .collect();
    if declared != registry {
        violations.push(format!(
            "{OWNED_GATE_FILE}: the `{PROVIDER_FEATURE_ENUM}` variant set {declared:?} does not \
             equal the gated-feature registry {registry:?} — a new carrier feature method must \
             land WITH a ProviderFeature variant AND a gate (and vice versa)"
        ));
    }

    // (1 cont. / 2) Every gated feature method routes through a live in-body
    //     `feature_admits(` CALL and names its `ProviderFeature::<Variant>` — the static
    //     shape of the conditional OWNED delegation. The RUNTIME dominance (denied ⇒ no
    //     OWNED delegation) is owned by owned_binding_gate.rs (see the module comment above).
    for (method, variant) in GATED_FEATURE_METHODS {
        match method_body_span(&composite_src, Some(COMPOSITE_FEATURE_IMPL), method) {
            None => violations.push(format!(
                "{OWNED_GATE_FILE}: could not locate the gated feature method `{method}` on \
                 `impl {COMPOSITE_FEATURE_IMPL}` — the admission-gate anchor is stale"
            )),
            Some(span) => {
                if !calls_named_fn_in_span(&composite_src, FEATURE_ADMIT_FN, span) {
                    violations.push(format!(
                        "{OWNED_GATE_FILE}: the carrier feature method `{method}` must route through \
                         a live `{FEATURE_ADMIT_FN}(` admission CALL inside its own body before \
                         delegating to OWNED — no in-body gate call found (an ungated feature is a \
                         `--lsp` self-discovery fall-through)"
                    ));
                }
                let variant_ref = format!("{PROVIDER_FEATURE_ENUM}::{variant}");
                if !live_needle_in_span(&composite_src, &variant_ref, span) {
                    violations.push(format!(
                        "{OWNED_GATE_FILE}: the carrier feature method `{method}` must name its \
                         `{variant_ref}` variant inside its body (the method↔variant tie)"
                    ));
                }
            }
        }
    }

    // (1 cont. / 2) EXHAUSTIVE DELEGATION PARTITION: over EVERY `self.owned.*` call in the
    //     trait impl (not just the registry rows), require each enclosing method to be a
    //     gated feature whose delegation is DOMINATED by `feature_admits(` OR an allowlisted
    //     lifecycle/passthrough method. This is the true-exhaustiveness half: a NEW ungated
    //     feature method (the get_code_actions class, or any future one) is caught even if it
    //     never lands in `GATED_FEATURE_METHODS`, and a gated method whose gate does not
    //     precede the OWNED call is caught too.
    violations.extend(owned_delegation_partition_violations(&composite_src));

    // (3) Plain `.ts`/`.tsx` are NOT gated: `feature_admits` classifies the carrier companion
    //     via `carrier_source_of` (a non-carrier returns ungated) and consults the admission
    //     cache via `admit(`.
    match method_body_span(&composite_src, Some(OWNED_GATE_IMPL), FEATURE_ADMIT_FN) {
        None => violations.push(format!(
            "{OWNED_GATE_FILE}: could not locate the `{FEATURE_ADMIT_FN}` helper body on \
             `impl {OWNED_GATE_IMPL}` — the ungated-non-carrier anchor is stale"
        )),
        Some(span) => {
            if !calls_named_fn_in_span(&composite_src, "carrier_source_of", span) {
                violations.push(format!(
                    "{OWNED_GATE_FILE}: the `{FEATURE_ADMIT_FN}` helper must CALL `carrier_source_of(` \
                     so a NON-carrier `.ts`/`.tsx` is left UNGATED (only a carrier companion is gated)"
                ));
            }
            if !calls_named_fn_in_span(&composite_src, "admit", span) {
                violations.push(format!(
                    "{OWNED_GATE_FILE}: the `{FEATURE_ADMIT_FN}` helper must consult the admission \
                     cache via a live `admit(` call"
                ));
            }
        }
    }

    // (4) Feature admission rides the ONE shared binding source: the CarrierAdmissionCache in
    //     project_binding.rs resolves through `resolve_carrier_bound(` (never a feature-local
    //     fork), and the shared published_root → WorkspaceProjectResolver → ProjectBinding →
    //     ensure_project → BoundProject chain is intact.
    if !calls_named_fn(&binding_src, "resolve_carrier_bound") {
        violations.push(format!(
            "{PROJECT_BINDING_FILE}: the CarrierAdmissionCache must resolve through a live \
             `resolve_carrier_bound(` call (the ONE shared binding source), never a feature-local fork"
        ));
    }
    for needle in [
        "published_root",
        "WorkspaceProjectResolver",
        "ensure_project",
        "BoundProject",
    ] {
        if !has_live(&binding_src, needle) {
            violations.push(format!(
                "{PROJECT_BINDING_FILE}: the shared binding chain lost `{needle}` — feature \
                 admission must ride the published_root → WorkspaceProjectResolver → ProjectBinding \
                 → ensure_project → BoundProject source"
            ));
        }
    }

    // (5) The `--api` oracle stays test-only: no carrier feature route in composite.rs calls
    //     it (the main test bans it across all production src; here the feature file is pinned
    //     explicitly).
    if calls_named_fn(
        &composite_src,
        "semantic_diagnostics_for_carrier_in_project",
    ) {
        violations.push(format!(
            "{OWNED_GATE_FILE}: a carrier feature route calls the test-only `--api` oracle \
             `semantic_diagnostics_for_carrier_in_project` — a raw path-only bypass of the \
             BoundProject admission layer"
        ));
    }

    assert!(
        violations.is_empty(),
        "carrier FEATURE admission-gate violations (features ride the SAME BoundProject admission \
         as diagnostics, never an ungated `--lsp` fall-through):\n{}",
        violations.join("\n")
    );
}

/// DISCRIMINATING self-test for the SB6c-5 feature-gate predicates: `enum_variants` extracts
/// fieldless variants; a properly-gated synthetic method satisfies the gate-call + variant-name
/// checks; a PLANTED UNGATED method (delegating to `self.owned` WITHOUT `feature_admits`) FAILS
/// the gate-call check (the core discrimination — a new ungated feature ⇒ guard RED); an
/// exhaustiveness mismatch is detected; and the LIVE composite.rs satisfies the whole gate for
/// every registry method (non-vacuous on the tree).
#[test]
fn carrier_feature_gate_guard_discriminates() {
    // `enum_variants` extracts fieldless variants, skipping doc + section comments.
    let enum_src =
        "enum ProviderFeature {\n    /// doc\n    Hover,\n    // ── section ──\n    Definition,\n}\n";
    assert_eq!(
        enum_variants(enum_src, "ProviderFeature"),
        vec!["Hover".to_string(), "Definition".to_string()],
        "enum_variants pulls the PascalCase variants and skips comments"
    );
    assert!(
        enum_variants("struct X;\n", "ProviderFeature").is_empty(),
        "a missing enum yields no variants (never a false positive)"
    );

    // A properly-gated synthetic method satisfies BOTH brace-scoped checks.
    let gated = "impl TypeProvider for TsgoCompositeProvider {\n    \
         fn get_hover(&self, path: &str, offset: u32) -> F {\n        \
         let path = path.to_string();\n        Box::pin(async move {\n            \
         if self.feature_admits(ProviderFeature::Hover, &path) {\n                \
         self.owned.get_hover(&path, offset).await\n            } else {\n                \
         Ok(None)\n            }\n        })\n    }\n}\n";
    let gated_span = method_body_span(gated, Some(COMPOSITE_FEATURE_IMPL), "get_hover")
        .expect("gated method body span");
    assert!(
        calls_named_fn_in_span(gated, FEATURE_ADMIT_FN, gated_span),
        "a gated method satisfies the in-body feature_admits( gate-call check"
    );
    assert!(
        live_needle_in_span(gated, "ProviderFeature::Hover", gated_span),
        "a gated method names its ProviderFeature::Hover variant in-body"
    );

    // A PLANTED UNGATED method (delegates to self.owned WITHOUT feature_admits) must FAIL the
    // gate-call check — the core discrimination (a new ungated feature ⇒ guard RED).
    let ungated = "impl TypeProvider for TsgoCompositeProvider {\n    \
         fn get_hover(&self, path: &str, offset: u32) -> F {\n        \
         self.owned.get_hover(path, offset)\n    }\n}\n";
    let ungated_span = method_body_span(ungated, Some(COMPOSITE_FEATURE_IMPL), "get_hover")
        .expect("ungated method body span");
    assert!(
        !calls_named_fn_in_span(ungated, FEATURE_ADMIT_FN, ungated_span),
        "a PLANTED UNGATED feature method must NOT satisfy the gate-call check (guard RED) — the \
         whole point of the strengthening"
    );

    // ── DELEGATION PARTITION discrimination ──

    // (catch-all) A NEW ungated method NOT in the registry AND NOT allowlisted delegates
    // to `self.owned` — the partition FLAGS it (the true-exhaustiveness property: a future
    // ungated feature is caught even without a registry row).
    let unregistered = "impl TypeProvider for TsgoCompositeProvider {\n    \
         fn get_brand_new_feature(&self, path: &str) -> F {\n        \
         self.owned.get_brand_new_feature(path)\n    }\n}\n";
    let unregistered_violations = owned_delegation_partition_violations(unregistered);
    assert!(
        unregistered_violations
            .iter()
            .any(|v| v.contains("get_brand_new_feature")),
        "an unregistered, un-allowlisted `self.owned` delegation must be FLAGGED by the partition \
         (guard RED) — the whole point of the strengthening; got {unregistered_violations:?}"
    );

    // (dominance) A REGISTERED gated method whose `feature_admits(` gate comes AFTER the
    // `self.owned` delegation (not dominating) is FLAGGED — the gate must precede the call.
    let undominated = "impl TypeProvider for TsgoCompositeProvider {\n    \
         fn get_hover(&self, path: &str, offset: u32) -> F {\n        \
         let r = self.owned.get_hover(path, offset);\n        \
         if self.feature_admits(ProviderFeature::Hover, path) { r } else { none }\n    }\n}\n";
    let undominated_violations = owned_delegation_partition_violations(undominated);
    assert!(
        undominated_violations
            .iter()
            .any(|v| v.contains("get_hover") && v.contains("dominating")),
        "a gated method whose gate does NOT precede the delegation must be FLAGGED (guard RED); \
         got {undominated_violations:?}"
    );

    // (dominance — empty-guard evasion) A REGISTERED gated method whose `feature_admits(`
    // gate PRECEDES the `self.owned` delegation lexically but does NOT GUARD it — an empty
    // `if feature_admits(..) {}` then an ungated `self.owned` call at the SAME brace depth —
    // must be FLAGGED. Mere token-order (`admit` before `owned`) is not dominance; the
    // strengthened depth check requires the OWNED call INSIDE the guarded block.
    let empty_guard_evasion = "impl TypeProvider for TsgoCompositeProvider {\n    \
         fn get_hover(&self, path: &str, offset: u32) -> F {\n        \
         if self.feature_admits(ProviderFeature::Hover, path) {}\n        \
         self.owned.get_hover(path, offset)\n    }\n}\n";
    let evasion_violations = owned_delegation_partition_violations(empty_guard_evasion);
    assert!(
        evasion_violations
            .iter()
            .any(|v| v.contains("get_hover") && v.contains("GUARDING")),
        "an empty `if feature_admits(){{}}` followed by an ungated `self.owned` call (gate \
         precedes but does not GUARD the delegation) must be FLAGGED by the strengthened \
         brace-depth check (guard RED) — token-order is not control-flow dominance; got \
         {evasion_violations:?}"
    );

    // (clean) A properly-gated feature (gate dominates) PLUS an allowlisted lifecycle method
    // yields NO partition violations.
    let clean = "impl TypeProvider for TsgoCompositeProvider {\n    \
         fn get_hover(&self, path: &str, offset: u32) -> F {\n        \
         if self.feature_admits(ProviderFeature::Hover, path) {\n            \
         self.owned.get_hover(path, offset)\n        } else { none }\n    }\n    \
         fn shutdown(&self) -> F {\n        self.owned.shutdown()\n    }\n}\n";
    assert!(
        owned_delegation_partition_violations(clean).is_empty(),
        "a dominated gated feature + an allowlisted lifecycle delegation must NOT be flagged"
    );

    // (stale anchor) A source with NO matching impl block is a non-vacuous FAILURE (the
    // partition must not silently pass when it can enumerate nothing).
    assert!(
        !owned_delegation_partition_violations("struct X;\n").is_empty(),
        "a missing impl block must be a partition violation (never a vacuous pass)"
    );

    // Exhaustiveness mismatch: a registry missing a declared variant is detected.
    let declared: BTreeSet<String> = enum_variants(
        "enum ProviderFeature {\n    Hover,\n    Extra,\n}\n",
        "ProviderFeature",
    )
    .into_iter()
    .collect();
    let partial: BTreeSet<String> = ["Hover".to_string()].into_iter().collect();
    assert_ne!(
        declared, partial,
        "a declared variant missing from the registry must break the set-equality tie"
    );

    // The LIVE composite.rs satisfies the whole gate for EVERY registry method — the
    // strengthened tail test is non-vacuous on the real tree.
    let composite_src = fs::read_to_string(workspace_root().join(OWNED_GATE_FILE))
        .expect("read the live composite.rs");
    let declared_live: BTreeSet<String> = enum_variants(&composite_src, PROVIDER_FEATURE_ENUM)
        .into_iter()
        .collect();
    let registry_live: BTreeSet<String> = GATED_FEATURE_METHODS
        .iter()
        .map(|(_, v)| (*v).to_string())
        .collect();
    assert_eq!(
        declared_live, registry_live,
        "the live ProviderFeature variant set must equal the gated-feature registry"
    );
    // The live trait impl's delegation partition is CLEAN — every `self.owned.*` call is a
    // dominated gated feature or an allowlisted lifecycle method (non-vacuous GREEN on the
    // real tree, and proof the allowlist + registry are COMPLETE for it).
    assert!(
        owned_delegation_partition_violations(&composite_src).is_empty(),
        "the live composite.rs delegation partition must be clean: {:?}",
        owned_delegation_partition_violations(&composite_src)
    );
    for (method, variant) in GATED_FEATURE_METHODS {
        let span = method_body_span(&composite_src, Some(COMPOSITE_FEATURE_IMPL), method)
            .unwrap_or_else(|| panic!("live composite.rs must expose the gated method `{method}`"));
        assert!(
            calls_named_fn_in_span(&composite_src, FEATURE_ADMIT_FN, span),
            "live `{method}` must gate via an in-body feature_admits( call"
        );
        assert!(
            live_needle_in_span(
                &composite_src,
                &format!("{PROVIDER_FEATURE_ENUM}::{variant}"),
                span
            ),
            "live `{method}` must name ProviderFeature::{variant} in-body"
        );
    }
}
