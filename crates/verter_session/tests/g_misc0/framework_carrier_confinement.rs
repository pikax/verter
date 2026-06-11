//! LEGACY_GATE_SELF — framework parse-carrier confinement guards.
//!
//! Three static architecture guards for the framework parse-artifact
//! substrate (`FrameworkParseArtifact` + `CarrierParse` +
//! `CarrierAccessToken`):
//!
//!  * `parsed_sfc_confined_to_vue_bridge` — production references to
//!    BOTH Vue parse tokens — the `ParsedSfc` type AND the `parse_sfc`
//!    producer function (a type-inferred `let parsed = parse_sfc(…)`
//!    call carries no `ParsedSfc` token and would escape a
//!    type-reference-only scan) — appear ONLY in the Vue-owned bridge
//!    allowlist: `crates/verter_parser/**`, `crates/verter_compiler/**`
//!    (the Vue compiler home incl. `framework_common/vue_bridge.rs`),
//!    and the named `verter_session` Vue-bridge files (`parse.rs`,
//!    `host_resolve/vue_script_extract.rs`,
//!    `typeinfo/adapters/vue/**`). Everything else reaches Vue parse
//!    data through the blessed `vue_parse()` /
//!    `carrier_for::<VueParseCarrier>` accessors.
//!
//!  * `carrier_downcast_confined_to_owning_adapter` — the raw
//!    `#[doc(hidden)] __carrier_downcast_ref` / `__carrier_downcast_arc`
//!    helpers appear ONLY in `verter_language::parse_artifact`,
//!    `verter_session/src/framework/ctx.rs`, and
//!    `verter_compiler/src/framework_common/ctx.rs` (the blessed
//!    `carrier_for::<T>` wrapper homes); typed carrier use
//!    (`VueParseCarrier`) only inside the owning adapter/compiler
//!    paths.
//!
//!  * `carrier_access_token_minted_only_in_verter_language` —
//!    `CarrierAccessToken` construction expressions appear ONLY in the
//!    owning `verter_language` minting files (`parse_artifact.rs` +
//!    the `LanguageRegistry` row-construction module `registry.rs`);
//!    the API-surface half pins that NO public arbitrary-id
//!    constructor and NO public by-id token lookup exist — descriptors
//!    and `vue_parse()` RECEIVE the token, never construct it.
//!
//! Scanner discipline mirrors `no_legacy_walker.rs`: production
//! `crates/*/src/**/*.rs` only, comments + string literals + inline
//! `#[cfg(test)]` modules stripped, `LEGACY_GATE_SELF`-marked files
//! skipped, identifier-boundary token matching.

use std::path::{Path, PathBuf};

/// Files whose head carries `LEGACY_GATE_SELF` are scanner code.
const SELF_MARKER: &str = "LEGACY_GATE_SELF";

// ───────────────────────────── allowlists ─────────────────────────────

/// The Vue-owned bridge allowlist for the `ParsedSfc` / `parse_sfc`
/// tokens (path prefixes relative to the workspace root, `/`-separated).
const VUE_BRIDGE_ALLOWLIST: &[&str] = &[
    // The parser crate owns the type.
    "crates/verter_parser/",
    // The Vue compiler home (incl. framework_common/vue_bridge.rs).
    "crates/verter_compiler/",
    // The named verter_session Vue-bridge files.
    "crates/verter_session/src/parse.rs",
    "crates/verter_session/src/host_resolve/vue_script_extract.rs",
    "crates/verter_session/src/typeinfo/adapters/vue/",
];

/// The blessed raw-downcast homes (`__carrier_downcast_ref` /
/// `__carrier_downcast_arc` may appear nowhere else). The owning
/// crate's `lib.rs` is allowlisted for its `pub use` export line only —
/// it is the definition's export surface, not a consumer.
const RAW_DOWNCAST_ALLOWLIST: &[&str] = &[
    "crates/verter_language/src/parse_artifact.rs",
    "crates/verter_language/src/lib.rs",
    "crates/verter_session/src/framework/ctx.rs",
    "crates/verter_compiler/src/framework_common/ctx.rs",
];

/// Typed Vue carrier (`VueParseCarrier`) use is confined to the owning
/// adapter/compiler paths.
const TYPED_CARRIER_ALLOWLIST: &[&str] = &[
    "crates/verter_compiler/src/framework_common/vue_bridge.rs",
    "crates/verter_session/src/typeinfo/adapters/vue/",
];

/// `CarrierAccessToken` construction expressions (struct literals and
/// the crate-private factory) are confined to the `verter_language`
/// minting files.
const TOKEN_MINTING_ALLOWLIST: &[&str] = &[
    "crates/verter_language/src/parse_artifact.rs",
    "crates/verter_language/src/registry.rs",
];

/// The carrier-row REGISTRATION channel (`LanguageRow::carrier` /
/// `LanguageRegistry::__built_in_with_carrier_tokens`) is itself
/// confined: tokens flow only to verter_language internals and the
/// session's sanctioned receipt site (the Vue adapter's
/// `receive_vue_carrier_token`). Any other production call site would
/// let an arbitrary crate mint an adapter's token through the public
/// row constructor — the same forging vector D-ba bans for a public
/// arbitrary-id constructor. The adapter-registry descriptor
/// construction extends this list when it lands.
const TOKEN_RECEIPT_ALLOWLIST: &[&str] = &[
    "crates/verter_language/src/",
    "crates/verter_session/src/typeinfo/adapters/vue/parse_access.rs",
];

// ───────────────────────────── the guards ─────────────────────────────

#[test]
fn parsed_sfc_confined_to_vue_bridge() {
    let violations =
        scan_tokens_outside_allowlist(&["ParsedSfc", "parse_sfc"], VUE_BRIDGE_ALLOWLIST);
    assert!(
        violations.is_empty(),
        "Vue parse tokens (`ParsedSfc` type / `parse_sfc` producer call) must stay \
         confined to the Vue bridge allowlist {VUE_BRIDGE_ALLOWLIST:?}; all other \
         production code reaches Vue parse data through the blessed `vue_parse()` \
         accessor over `FrameworkParseArtifact`. Violations:\n{violations:#?}"
    );
}

#[test]
fn carrier_downcast_confined_to_owning_adapter() {
    // The raw token-gated downcast helpers.
    let raw_violations = scan_tokens_outside_allowlist(
        &["__carrier_downcast_ref", "__carrier_downcast_arc"],
        RAW_DOWNCAST_ALLOWLIST,
    );
    assert!(
        raw_violations.is_empty(),
        "the raw `__carrier_downcast_ref`/`__carrier_downcast_arc` helpers may appear \
         ONLY in the blessed wrapper homes {RAW_DOWNCAST_ALLOWLIST:?}; every other \
         consumer routes through `carrier_for::<T>`. Violations:\n{raw_violations:#?}"
    );

    // Typed carrier use stays inside the owning adapter/compiler paths.
    let typed_violations =
        scan_tokens_outside_allowlist(&["VueParseCarrier"], TYPED_CARRIER_ALLOWLIST);
    assert!(
        typed_violations.is_empty(),
        "`VueParseCarrier` (the typed Vue carrier) may be used ONLY inside the owning \
         adapter/compiler paths {TYPED_CARRIER_ALLOWLIST:?}. Violations:\n{typed_violations:#?}"
    );
}

#[test]
fn carrier_access_token_minted_only_in_verter_language() {
    // (1) Construction-expression confinement: a `CarrierAccessToken`
    // struct literal (`CarrierAccessToken {`) or a call to the
    // crate-private factory may appear ONLY in the minting files.
    let mut violations = scan_predicate_outside_allowlist(
        |line| line_has_token_struct_literal(line, "CarrierAccessToken"),
        TOKEN_MINTING_ALLOWLIST,
        "CarrierAccessToken struct literal",
    );
    violations.extend(scan_tokens_outside_allowlist(
        &["mint_carrier_access_token"],
        TOKEN_MINTING_ALLOWLIST,
    ));
    // The registration channel is confined to the sanctioned receipt
    // sites: a call to `LanguageRow::carrier` or
    // `__built_in_with_carrier_tokens` anywhere else is a forged-token
    // vector through the public row constructor.
    violations.extend(scan_predicate_outside_allowlist(
        |line| line.contains("LanguageRow::carrier"),
        TOKEN_RECEIPT_ALLOWLIST,
        "LanguageRow::carrier registration-channel call",
    ));
    violations.extend(scan_tokens_outside_allowlist(
        &["__built_in_with_carrier_tokens"],
        TOKEN_RECEIPT_ALLOWLIST,
    ));
    assert!(
        violations.is_empty(),
        "`CarrierAccessToken` construction expressions are confined to the \
         `verter_language` minting files {TOKEN_MINTING_ALLOWLIST:?}, and the \
         registration channel to the sanctioned receipt sites \
         {TOKEN_RECEIPT_ALLOWLIST:?} (D-ba: `verter_language` is the SOLE minting \
         authority — the token is minted during `LanguageRegistry` carrier-row \
         construction and returned exactly once as the carrier row's registration \
         proof). Violations:\n{violations:#?}"
    );

    // (2) API-surface half: NO public arbitrary-id constructor
    // (`new(adapter_id)` / `From` / `Default`) and NO public by-id
    // token lookup exist. The ONLY functions in `verter_language`
    // whose return type carries `CarrierAccessToken` are the
    // carrier-row registration channel.
    let root = workspace_root();
    let lang_src = root.join("crates/verter_language/src");
    let mut files = Vec::new();
    collect_production_rs(&lang_src, &mut files);
    let mut offending: Vec<String> = Vec::new();
    let allowed_fns = [
        "carrier",
        "__built_in_with_carrier_tokens",
        "mint_carrier_access_token",
    ];
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let processed = preprocess(&text);
        // Forbid From / Default / TryFrom impls for the token.
        for forbidden in [
            "impl From<",
            "impl Default for CarrierAccessToken",
            "impl TryFrom<",
        ] {
            for (idx, line) in processed.lines().enumerate() {
                if line.contains(forbidden) && line.contains("CarrierAccessToken") {
                    offending.push(format!(
                        "{}:{}: forbidden conversion impl `{}`",
                        file.display(),
                        idx + 1,
                        line.trim()
                    ));
                }
            }
        }
        // Every fn whose signature RETURNS the token must be one of the
        // registration-proof channel functions.
        for (name, line_no, line) in fns_returning_token(&processed) {
            if !allowed_fns.contains(&name.as_str()) {
                offending.push(format!(
                    "{}:{}: fn `{}` returns CarrierAccessToken outside the \
                     registration-proof channel: `{}`",
                    file.display(),
                    line_no,
                    name,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        offending.is_empty(),
        "CarrierAccessToken API surface violation (D-ba pins: no public arbitrary-id \
         constructor, no `From`/`Default`, no public by-id token lookup — the token is \
         returned only through carrier-row construction):\n{offending:#?}"
    );
}

// ─────────────────── negative self-tests (scanner discipline) ───────────────────

#[test]
fn scanner_detects_misplaced_parsed_sfc_type_reference() {
    // A `ParsedSfc` TYPE reference in synthetic production source must
    // be detected by the token scanner.
    let synthetic = "\
pub(crate) fn misplaced(parsed: &verter_compiler::parser::types::ParsedSfc) {\n\
    let _ = parsed;\n\
}\n";
    let processed = preprocess(synthetic);
    assert!(
        processed
            .lines()
            .any(|l| line_contains_identifier(l, "ParsedSfc")),
        "scanner must detect a misplaced ParsedSfc type reference"
    );
}

#[test]
fn scanner_detects_misplaced_bare_parse_sfc_call() {
    // D-bb: a type-inferred `let parsed = parse_sfc(…)` call carries NO
    // `ParsedSfc` token — the scanner must catch the producer-call
    // token itself.
    let synthetic = "\
pub(crate) fn misplaced(source: &str) {\n\
    let parsed = verter_compiler::compile::parse_sfc(source, None, None);\n\
    let _ = parsed;\n\
}\n";
    let processed = preprocess(synthetic);
    assert!(
        !processed
            .lines()
            .any(|l| line_contains_identifier(l, "ParsedSfc")),
        "the bare-call fixture must NOT carry the type token (that is its point)"
    );
    assert!(
        processed
            .lines()
            .any(|l| line_contains_identifier(l, "parse_sfc")),
        "scanner must detect a misplaced bare parse_sfc(…) producer call"
    );
}

#[test]
fn scanner_detects_misplaced_raw_downcast_and_typed_carrier() {
    let synthetic = "\
fn sneaky(artifact: &FrameworkParseArtifact, token: &CarrierAccessToken) {\n\
    let _ = verter_language::__carrier_downcast_ref::<VueParseCarrier>(artifact, token);\n\
}\n";
    let processed = preprocess(synthetic);
    assert!(
        processed
            .lines()
            .any(|l| line_contains_identifier(l, "__carrier_downcast_ref")),
        "scanner must detect a misplaced raw __carrier_downcast_ref call"
    );
    assert!(
        processed
            .lines()
            .any(|l| line_contains_identifier(l, "VueParseCarrier")),
        "scanner must detect misplaced typed-carrier use"
    );
}

#[test]
fn scanner_detects_misplaced_token_construction() {
    // Struct-literal form.
    let literal = "let t = CarrierAccessToken { adapter_id, _private: () };\n";
    assert!(
        preprocess(literal)
            .lines()
            .any(|l| line_has_token_struct_literal(l, "CarrierAccessToken")),
        "scanner must detect a misplaced CarrierAccessToken struct literal"
    );
    // A type ascription / parameter position is NOT a construction.
    let ascription = "fn takes(token: &CarrierAccessToken) {}\n";
    assert!(
        !preprocess(ascription)
            .lines()
            .any(|l| line_has_token_struct_literal(l, "CarrierAccessToken")),
        "a token parameter/ascription is not a construction expression"
    );
    // A return-type position is NOT a construction (the `{` opens the
    // fn body), nor is an `impl` header.
    let return_position = "fn token() -> &'static CarrierAccessToken {\n";
    assert!(
        !preprocess(return_position)
            .lines()
            .any(|l| line_has_token_struct_literal(l, "CarrierAccessToken")),
        "a return-type position is not a construction expression"
    );
    let impl_header = "impl CarrierAccessToken {\n";
    assert!(
        !preprocess(impl_header)
            .lines()
            .any(|l| line_has_token_struct_literal(l, "CarrierAccessToken")),
        "an impl header is not a construction expression"
    );
    // Factory-call form.
    let factory = "let t = mint_carrier_access_token(adapter_id);\n";
    assert!(
        preprocess(factory)
            .lines()
            .any(|l| line_contains_identifier(l, "mint_carrier_access_token")),
        "scanner must detect a misplaced factory call"
    );
    // Registration-channel forms: a misplaced row-construction call is
    // the public-constructor forging vector and must be caught.
    let row_channel = "let (_row, token) = LanguageRow::carrier(\"vue\", FileLanguage::vue());\n";
    assert!(
        preprocess(row_channel)
            .lines()
            .any(|l| l.contains("LanguageRow::carrier")),
        "scanner must detect a misplaced LanguageRow::carrier call"
    );
    let registry_channel =
        "let (_registry, tokens) = LanguageRegistry::__built_in_with_carrier_tokens();\n";
    assert!(
        preprocess(registry_channel)
            .lines()
            .any(|l| line_contains_identifier(l, "__built_in_with_carrier_tokens")),
        "scanner must detect a misplaced __built_in_with_carrier_tokens call"
    );
    // API-surface half: a synthetic public arbitrary-id constructor
    // must be caught by the return-type scan.
    let forged_ctor = "\
impl CarrierAccessToken {\n\
    pub fn new(adapter_id: FrameworkAdapterId) -> CarrierAccessToken {\n\
        mint_carrier_access_token(adapter_id)\n\
    }\n\
}\n";
    let fns = fns_returning_token(&preprocess(forged_ctor));
    assert!(
        fns.iter().any(|(name, _, _)| name == "new"),
        "the API-surface scan must catch a forged public arbitrary-id constructor; got {fns:?}"
    );
}

#[test]
fn scanner_ignores_tests_comments_and_identifier_extensions() {
    // Doc comments / line comments are stripped.
    let commented = "\
/// Mentions ParsedSfc and parse_sfc historically.\n\
// parse_sfc in a line comment.\n\
pub fn live() {}\n";
    let processed = preprocess(commented);
    assert!(
        !processed
            .lines()
            .any(|l| line_contains_identifier(l, "ParsedSfc")
                || line_contains_identifier(l, "parse_sfc")),
        "preprocess must erase comment references"
    );
    // Inline #[cfg(test)] modules are stripped.
    let inline_tests = "\
pub fn live() {}\n\
#[cfg(test)]\n\
mod tests {\n\
    fn touch() { let _ = parse_sfc(\"\", None, None); }\n\
}\n";
    assert!(
        !preprocess(inline_tests)
            .lines()
            .any(|l| line_contains_identifier(l, "parse_sfc")),
        "preprocess must erase #[cfg(test)] mod bodies"
    );
    // Identifier-boundary discipline: extended identifiers do not match.
    let extended = "let parse_sfc_artifact = build(); type ParsedSfcLike = ();\n";
    let processed = preprocess(extended);
    assert!(
        !processed
            .lines()
            .any(|l| line_contains_identifier(l, "parse_sfc")
                || line_contains_identifier(l, "ParsedSfc")),
        "identifier-boundary matcher must not match extended identifiers"
    );
    // Test files are excluded by path classification.
    assert!(is_test_file(Path::new("crates/x/src/foo_tests.rs")));
    assert!(is_test_file(Path::new("crates/x/tests/guard.rs")));
    assert!(!is_test_file(Path::new("crates/x/src/foo.rs")));
}

// ───────────────────────────── scan machinery ─────────────────────────────

/// A struct-literal construction expression: the token name followed
/// (on the same line, modulo whitespace) by `{`. Parameter positions,
/// ascriptions, paths (`CarrierAccessToken::`), `impl` headers, and
/// return-type positions (`-> … CarrierAccessToken {`, where the brace
/// opens the fn body) do not match.
fn line_has_token_struct_literal(line: &str, type_name: &str) -> bool {
    // `impl CarrierAccessToken {` / `impl X for CarrierAccessToken {`
    // open an impl block, not a literal.
    if line.trim_start().starts_with("impl") {
        return false;
    }
    let bytes = line.as_bytes();
    let needle = type_name.as_bytes();
    let n = needle.len();
    if bytes.len() < n {
        return false;
    }
    let is_ident_char = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let arrow = line.find("->");
    let mut i = 0usize;
    while i + n <= bytes.len() {
        if &bytes[i..i + n] == needle {
            let before_ok = i == 0 || !is_ident_char(bytes[i - 1]);
            let after = bytes.get(i + n).copied();
            let after_ok = after.is_none_or(|b| !is_ident_char(b));
            // A token after `->` on the same line sits in return-type
            // position — the `{` that follows opens the fn body.
            let in_return_position = arrow.is_some_and(|a| a < i);
            if before_ok && after_ok && !in_return_position {
                // Skip whitespace after the token; a `{` means literal.
                let mut k = i + n;
                while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\t') {
                    k += 1;
                }
                if bytes.get(k) == Some(&b'{') {
                    return true;
                }
            }
        }
        i += 1;
    }
    false
}

/// Functions whose signature line carries `-> … CarrierAccessToken …`.
/// Returns `(fn_name, 1-based line, line text)`.
fn fns_returning_token(processed: &str) -> Vec<(String, usize, String)> {
    let mut out = Vec::new();
    let lines: Vec<&str> = processed.lines().collect();
    for (idx, line) in lines.iter().enumerate() {
        let Some(fn_pos) = find_fn_keyword(line) else {
            continue;
        };
        // Capture the fn name.
        let after = &line[fn_pos + 3..];
        let name: String = after
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            continue;
        }
        // Look at the signature: this line plus up to 8 continuation
        // lines (until `{` or `;`).
        let mut sig = String::new();
        for cont in lines.iter().skip(idx).take(9) {
            sig.push_str(cont);
            sig.push(' ');
            if cont.contains('{') || cont.contains(';') {
                break;
            }
        }
        if let Some(arrow) = sig.find("->") {
            let ret = &sig[arrow..];
            let ret_end = ret.find('{').unwrap_or(ret.len());
            if line_contains_identifier(&ret[..ret_end], "CarrierAccessToken") {
                out.push((name, idx + 1, line.to_string()));
            }
        }
    }
    out
}

/// Position of a standalone `fn ` keyword on the line.
fn find_fn_keyword(line: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i + 3 <= bytes.len() {
        if &bytes[i..i + 2] == b"fn"
            && bytes.get(i + 2) == Some(&b' ')
            && (i == 0 || !(bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_'))
        {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Scan all production sources for identifier-boundary token hits
/// outside the given allowlist of `/`-separated path prefixes.
fn scan_tokens_outside_allowlist(tokens: &[&str], allowlist: &[&str]) -> Vec<String> {
    scan_outside_allowlist(allowlist, |line| {
        tokens
            .iter()
            .find(|t| line_contains_identifier(line, t))
            .map(|t| t.to_string())
    })
}

/// Scan with an arbitrary per-line predicate (label used in reports).
fn scan_predicate_outside_allowlist(
    pred: impl Fn(&str) -> bool,
    allowlist: &[&str],
    label: &str,
) -> Vec<String> {
    scan_outside_allowlist(allowlist, |line| {
        if pred(line) {
            Some(label.to_string())
        } else {
            None
        }
    })
}

fn scan_outside_allowlist(
    allowlist: &[&str],
    classify: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    let root = workspace_root();
    let files = collect_production_sources();
    let mut violations = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        if allowlist.iter().any(|prefix| rel.starts_with(prefix)) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        if text.lines().take(5).any(|line| line.contains(SELF_MARKER)) {
            continue;
        }
        let processed = preprocess(&text);
        for (idx, line) in processed.lines().enumerate() {
            if let Some(what) = classify(line) {
                violations.push(format!("{rel}:{}: {what}: `{}`", idx + 1, line.trim()));
            }
        }
    }
    violations
}

// ─────────── shared scanner helpers (mirrors no_legacy_walker.rs) ───────────

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

/// Walk a `crates/*/src/` tree and collect every production `.rs` file.
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

/// Every production `.rs` file under `crates/*/src/`.
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

/// Replace `//` / `/* … */` comments AND string-literal contents with
/// whitespace (newlines preserved). String contents are blanked so a
/// token inside a trace/format string never counts as production use.
fn strip_comments_and_strings(src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let n = bytes.len();
    let mut i = 0usize;
    while i < n {
        let c = bytes[i];
        // Raw string r"..." / r#"..."# — blank the contents.
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
                let mut closed = false;
                while k + close.len() <= n {
                    if &bytes[k..k + close.len()] == close.as_slice() {
                        out.extend_from_slice(&close);
                        i = k + close.len();
                        closed = true;
                        break;
                    }
                    out.push(if bytes[k] == b'\n' { b'\n' } else { b' ' });
                    k += 1;
                }
                if !closed {
                    i = n;
                }
                continue;
            }
        }
        // Regular string literal "..." — blank the contents.
        if c == b'"' {
            out.push(b'"');
            let mut k = i + 1;
            while k < n {
                if bytes[k] == b'\\' && k + 1 < n {
                    out.push(b' ');
                    out.push(b' ');
                    k += 2;
                    continue;
                }
                if bytes[k] == b'"' {
                    out.push(b'"');
                    k += 1;
                    break;
                }
                out.push(if bytes[k] == b'\n' { b'\n' } else { b' ' });
                k += 1;
            }
            i = k;
            continue;
        }
        // Line comment.
        if c == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            let mut k = i;
            while k < n && bytes[k] != b'\n' {
                out.push(b' ');
                k += 1;
            }
            i = k;
            continue;
        }
        // Block comment with nesting.
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
                out.push(if bytes[k] == b'\n' { b'\n' } else { b' ' });
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
/// with whitespace (newlines preserved).
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
    strip_inline_test_modules(&strip_comments_and_strings(src))
}

/// Identifier-boundary matcher (mirrors `no_legacy_walker.rs`).
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
