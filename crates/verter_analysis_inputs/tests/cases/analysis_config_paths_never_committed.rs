//! Architecture guard: no local-analysis-input path, private project token, or
//! private absolute-path shape may appear in ANY committed artifact.
//!
//! Real-world projects are run through Verter as local analysis inputs; their
//! on-disk paths and identities are private. A leak vector is not only a source
//! file — it is a generated golden, a snapshot, a `.d.ts`, a JSON/JSONL event
//! stream, a source map's `sources`/`sourcesContent`, a log, the deviation ledger,
//! or a markdown doc. This guard enumerates every tracked file via
//! `git ls-files -z` and enforces two rules:
//!
//! 1. PATH-level: nothing tracked under `.analysis/` except exactly the one
//!    committable example; the live config is never tracked; `.gitignore` ignores
//!    the directory.
//! 2. CONTENT-level: every tracked TEXT artifact is scanned for the forbidden
//!    patterns. JSON / JSONL / source maps are PARSED and their DECODED string
//!    values scanned (so an escaped path inside a JSON string is caught), with a
//!    source map's `sources` + `sourcesContent` inspected explicitly.
//!
//! The private project tokens are NEVER spelled in tracked source — not literally
//! and not as reassemblable fragments. They are supplied out-of-band through the
//! `VERTER_PRIVATE_TOKENS` environment variable (comma- or newline-separated). When
//! the variable is unset the token class is skipped with a visible reason; the
//! path-level rule, the generic path-shape rule and the redactor-structure rule
//! need no private data and always run. A set-but-unusable value fails loudly.
//! Failure diagnostics identify a matched token by index only, never by value.
//! `.gitignore` is allowed to mention `.analysis/` (the directory name is not a
//! secret). The example config is path-allowed but still content-scanned.

use std::path::PathBuf;
use std::process::Command;

// ===========================================================================
// Private-token source (out-of-band; never tracked)
// ===========================================================================

/// Environment variable carrying the private project tokens.
const PRIVATE_TOKENS_ENV: &str = "VERTER_PRIVATE_TOKENS";

/// Parse a raw token list: comma- or newline-separated, trimmed, lower-cased,
/// slash-normalized (a path-combo token such as `a/b` is matched across any run
/// of either slash kind). Empty entries are dropped.
fn parse_private_tokens(raw: &str) -> Vec<String> {
    raw.split([',', '\n'])
        .map(|t| collapse_slashes(&normalize(t.trim()).to_ascii_lowercase()))
        .filter(|t| !t.is_empty())
        .collect()
}

/// The configured private tokens, or `None` when the variable is genuinely unset
/// (token class skipped). Set-but-unusable is a broken configuration: fail loudly
/// rather than silently skip.
fn private_tokens_from_env() -> Option<Vec<String>> {
    match std::env::var(PRIVATE_TOKENS_ENV) {
        Ok(raw) => {
            let tokens = parse_private_tokens(&raw);
            assert!(
                !tokens.is_empty(),
                "${PRIVATE_TOKENS_ENV} is set but lists no token; fix it or unset it to skip \
                 the private-token class"
            );
            Some(tokens)
        }
        Err(std::env::VarError::NotPresent) => {
            eprintln!(
                "SKIP private-token class: ${PRIVATE_TOKENS_ENV} is unset (the token list is \
                 never tracked); the rules that need no private data still ran"
            );
            None
        }
        Err(std::env::VarError::NotUnicode(_)) => panic!(
            "${PRIVATE_TOKENS_ENV} is set but not valid Unicode; fix it or unset it to skip \
             the private-token class"
        ),
    }
}

/// The banned tracked config-file path, assembled from fragments so it is never
/// spelled contiguously here. Used as DATA in the path-level rule.
fn banned_live_config_rel() -> String {
    format!("{}/{}", ".analysis", "projects.local.json")
}

/// The single allowed tracked path under `.analysis/`, assembled from fragments.
fn allowed_example_rel() -> String {
    format!("{}/{}", ".analysis", "projects.local.json.example")
}

// ===========================================================================
// The pure leak predicate — the discrimination test calls this directly.
// ===========================================================================

/// Forward-slash-normalize and lower-case for token + Windows-drive matching.
/// (Slash normalization makes the absolute-path shapes platform-independent.)
fn normalize(s: &str) -> String {
    s.replace('\\', "/")
}

/// Collapse every run of `/` to a single `/`, so a path-combo token matches
/// regardless of how many separators the leaked text carries.
fn collapse_slashes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_slash = false;
    for c in s.chars() {
        if c == '/' {
            if !prev_slash {
                out.push(c);
            }
            prev_slash = true;
        } else {
            out.push(c);
            prev_slash = false;
        }
    }
    out
}

/// True iff `c` can continue a path segment in the generic shapes (no slash, no
/// whitespace, no quote/angle-bracket terminators).
fn is_path_seg_char(c: char) -> bool {
    !(c.is_whitespace() || matches!(c, '/' | '"' | '\'' | '<' | '>'))
}

/// Generic private absolute-path shapes, over slash-normalized text:
///
/// - `\b[A-Za-z]:/(Users/<seg>|dev)/` — a Windows drive root into `Users/<name>` or `dev`
/// - `(^|[^A-Za-z0-9_])/(Users|home)/<name>/` — a POSIX home/Users root
/// - `file:///Users/...` and `file:///home/...`
///
/// Returns the first matched shape label, or `None`.
fn absolute_path_shape(norm: &str) -> Option<&'static str> {
    let bytes = norm.as_bytes();

    // file:///Users/... or file:///home/...
    {
        let lower = norm.to_ascii_lowercase();
        for marker in ["file:///users/", "file:///home/"] {
            if lower.contains(marker) {
                return Some("file-uri-home");
            }
        }
    }

    // [A-Za-z]:/(Users/<seg>|dev)/  — drive root. `\b` ⇒ the drive letter is not
    // preceded by another ASCII-alphanumeric/underscore.
    for (i, &b) in bytes.iter().enumerate() {
        if b.is_ascii_alphabetic() {
            // word-boundary before the drive letter
            if i > 0 {
                let prev = bytes[i - 1];
                if prev.is_ascii_alphanumeric() || prev == b'_' {
                    continue;
                }
            }
            let rest = &norm[i..];
            // `X:/`
            if rest.len() >= 3 && rest.as_bytes()[1] == b':' && rest.as_bytes()[2] == b'/' {
                let after = &rest[3..];
                // `dev/`
                let after_lower = after.to_ascii_lowercase();
                if after_lower.starts_with("dev/") {
                    return Some("drive-dev-root");
                }
                // `Users/<at least one seg char>` — require a real name segment.
                if after_lower.starts_with("users/") {
                    let name = &after[6..];
                    if name.chars().next().is_some_and(is_path_seg_char) {
                        return Some("drive-users-root");
                    }
                }
            }
        }
    }

    // (^|[^A-Za-z0-9_])/(Users|home)/<name>/  — POSIX root.
    let lower = norm.to_ascii_lowercase();
    let lbytes = lower.as_bytes();
    let mut search = 0;
    while search < lower.len() {
        // find next `/users/` or `/home/`
        let users_at = lower[search..]
            .find("/users/")
            .map(|r| (search + r, 7usize));
        let home_at = lower[search..].find("/home/").map(|r| (search + r, 6usize));
        let next = match (users_at, home_at) {
            (Some(a), Some(b)) => {
                if a.0 <= b.0 {
                    a
                } else {
                    b
                }
            }
            (Some(a), None) => a,
            (None, Some(b)) => b,
            (None, None) => break,
        };
        let (pos, kwlen) = next;
        // boundary before the leading `/`: start-of-string or a non-word char.
        let boundary_ok = pos == 0 || {
            let prev = lbytes[pos - 1];
            !(prev.is_ascii_alphanumeric() || prev == b'_')
        };
        if boundary_ok {
            // require a name segment then a `/`: `/(users|home)/<name>/`
            let after = &lower[pos + kwlen..];
            if let Some(slash_rel) = after.find('/') {
                let name = &after[..slash_rel];
                if !name.is_empty() && name.chars().all(is_path_seg_char) {
                    return Some("posix-home-root");
                }
            }
        }
        search = pos + 1;
    }

    None
}

/// CLASS 1 — the precise private-token predicate over already-DECODED text. Runs
/// TREE-WIDE over every tracked text artifact: a private project token is a leak
/// wherever it appears. `tokens` are already lower-cased + slash-collapsed (see
/// [`parse_private_tokens`]). Returns a reason naming the matched token BY INDEX
/// ONLY (a diagnostic must never echo a private token), or `None`.
pub fn token_leak(decoded_text: &str, tokens: &[String]) -> Option<String> {
    let haystack = collapse_slashes(&normalize(decoded_text).to_ascii_lowercase());
    tokens
        .iter()
        .position(|token| haystack.contains(token.as_str()))
        .map(|idx| format!("private token #{idx}"))
}

/// CLASS 2 — the generic private absolute-path-SHAPE predicate over already-DECODED
/// text. Runs ONLY on campaign-produced / generated-artifact surfaces (see
/// [`is_shape_scanned_artifact`]): the shape rules are intentionally conservative
/// and would false-positive on the synthetic `/Users/foo` path/URI test fixtures
/// and the dev's own `D:/dev/...` doc paths that legitimately live tree-wide. They
/// are a producer-boundary net for a path that slips the precise-token rules in an
/// EMITTED artifact, not a whole-tree path linter. Returns a reason, or `None`.
pub fn path_shape_leak(decoded_text: &str) -> Option<String> {
    let norm = normalize(decoded_text);
    if let Some(label) = absolute_path_shape(&norm) {
        return Some(format!("private absolute-path shape ({label})"));
    }
    None
}

/// The single explicit predicate naming the paths the CLASS 2 generic shape rules
/// run on: MACHINE-GENERATED / campaign-produced surfaces where a real path can ride
/// out and the precise-token rules would miss a path SHAPE.
///
/// Scoped IN: the committed example config; the deviation-ledger files; committed
/// `.map` source maps and `.jsonl` event streams; committed `.snap` snapshots; and
/// `CHANGELOG.md` — generated by git-cliff FROM COMMIT SUBJECTS, so a subject naming
/// a corpus path is a genuine leak vector the token-only cliff.toml redactor would
/// miss if the path is a SHAPE rather than a known token.
///
/// Scoped OUT (covered only by the tree-wide CLASS 1 token rules, by design):
/// hand-authored `.rs`/`.ts`/`.txt`, hand-authored docs (`docs/**.md`), and URI/path
/// test fixtures — these legitimately carry the dev's own `D:/dev/...` repo path and
/// synthetic `/Users/foo` examples, which are NOT corpus leaks and would false-
/// positive the conservative shape rules. The off-limits `verter_session` perf
/// goldens stay out (signed-off follow-up for that owner).
///
/// FUTURE: when a later campaign block introduces a new output/golden/log directory
/// that emits real paths, register its path prefix here.
pub fn is_shape_scanned_artifact(path: &str) -> bool {
    let p = path.replace('\\', "/");
    let lower = p.to_ascii_lowercase();

    // The committed example config (content-scanned; must stay clean).
    if p == allowed_example_rel() {
        return true;
    }
    // The deviation-ledger files (campaign-owned, machine + human).
    if p == "scripts/manifests/replacement-deviations.json"
        || p == "scripts/manifests/replacement-deviations.md"
    {
        return true;
    }
    // CHANGELOG.md is MACHINE-GENERATED from commit subjects — a genuine path-leak
    // vector. The cliff.toml postprocess redacts known TOKENS, not arbitrary path
    // SHAPES, so a corpus path shape in a subject must be caught here.
    if p == "CHANGELOG.md" {
        return true;
    }
    // Committed source maps and JSONL event streams are prime producer-boundary
    // leak vectors (a real path can ride a map's `sources` or a JSONL event).
    if lower.ends_with(".map") || lower.ends_with(".jsonl") {
        return true;
    }
    // Committed snapshots.
    if lower.ends_with(".snap") {
        return true;
    }
    // FUTURE campaign artifact dirs MUST be registered here when introduced
    // (e.g. a `.analysis/`-mirrored committed output dir, a campaign golden dir,
    // or a campaign log dir). They are not present yet.
    false
}

// ===========================================================================
// Tracked-tree enumeration + text detection.
// ===========================================================================

fn repo_root() -> PathBuf {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run `git rev-parse --show-toplevel`");
    assert!(out.status.success(), "git rev-parse failed");
    PathBuf::from(String::from_utf8(out.stdout).expect("utf8").trim_end())
}

/// Every tracked path, decoded as UTF-8 (the portability guard already enforces
/// UTF-8 tracked paths, so a lossy decode is safe here).
fn tracked_paths(root: &std::path::Path) -> Vec<String> {
    let out = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
        .expect("run `git ls-files -z`");
    assert!(out.status.success(), "git ls-files -z failed");
    out.stdout
        .split(|&b| b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect()
}

/// Extensions we treat as TEXT (scanned). Anything else is scanned ONLY if it has
/// no NUL byte in its first chunk (the binary sniff), so a stray extensionless
/// text artifact is still covered while a real binary is skipped.
const TEXT_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "mjs", "cjs", "mts", "cts", "vue", "svelte", "json", "jsonl",
    "map", "md", "mdx", "txt", "log", "yaml", "yml", "toml", "html", "css", "scss", "d.ts", "snap",
    "csv",
];

/// Decide text vs binary by extension allowlist, then a NUL-byte sniff fallback.
fn is_text(path: &str, bytes: &[u8]) -> bool {
    let lower = path.to_ascii_lowercase();
    if TEXT_EXTS.iter().any(|e| lower.ends_with(&format!(".{e}"))) {
        return true;
    }
    // No recognized text extension: treat as text unless an early NUL byte says binary.
    let head = &bytes[..bytes.len().min(8192)];
    !head.contains(&0)
}

// ===========================================================================
// Decoded-string collection for JSON / source maps.
// ===========================================================================

/// Recursively collect every string VALUE (and object KEY) from a parsed JSON
/// value into `out`. This catches a path hidden inside an escaped JSON string,
/// which a raw-byte scan of the file might miss (escaped slashes, unicode escapes).
fn collect_json_strings(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(a) => {
            for v in a {
                collect_json_strings(v, out);
            }
        }
        serde_json::Value::Object(o) => {
            for (k, v) in o {
                out.push(k.clone());
                collect_json_strings(v, out);
            }
        }
        _ => {}
    }
}

/// Check one decoded text fragment under the two-class scope for `path`: CLASS 1
/// (private tokens) ALWAYS; CLASS 2 (generic path shapes) only when `path` is a
/// shape-scanned campaign/generated artifact.
fn check_fragment(path: &str, text: &str, tokens: &[String]) -> Option<String> {
    if let Some(reason) = token_leak(text, tokens) {
        return Some(reason);
    }
    if is_shape_scanned_artifact(path) {
        if let Some(reason) = path_shape_leak(text) {
            return Some(reason);
        }
    }
    None
}

/// Scan one tracked TEXT artifact's content. Returns the first leak reason found.
///
/// The raw text is scanned (EOL-normalized to LF so a CRLF file is treated as
/// text). For JSON / JSONL / `.map`, the content is additionally PARSED and each
/// decoded string value scanned; for a source map the `sources` + `sourcesContent`
/// arrays are decoded as part of the whole-document string collection. The
/// two-class scope is applied per fragment via [`check_fragment`], so the generic
/// shape rules only fire on the shape-scanned artifact surfaces.
fn scan_text_artifact(path: &str, content: &str, tokens: &[String]) -> Option<String> {
    // Normalize EOL so the scan never depends on raw CR bytes.
    let text = content.replace("\r\n", "\n");

    // 1. Raw scan of the whole text (CLASS 1 always; CLASS 2 if shape-scanned).
    if let Some(reason) = check_fragment(path, &text, tokens) {
        return Some(reason);
    }

    let lower = path.to_ascii_lowercase();
    let is_json = lower.ends_with(".json") || lower.ends_with(".map");
    let is_jsonl = lower.ends_with(".jsonl");

    // 2. Decode JSON string values (catches escaped paths inside string literals,
    // incl. a source map's `sources` / `sourcesContent` entries).
    if is_json {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
            let mut strings = Vec::new();
            collect_json_strings(&value, &mut strings);
            for s in &strings {
                if let Some(reason) = check_fragment(path, s, tokens) {
                    return Some(format!("{reason} (decoded JSON string)"));
                }
            }
        }
    }
    // 3. JSONL: one JSON document per line.
    if is_jsonl {
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                let mut strings = Vec::new();
                collect_json_strings(&value, &mut strings);
                for s in &strings {
                    if let Some(reason) = check_fragment(path, s, tokens) {
                        return Some(format!("{reason} (decoded JSONL string)"));
                    }
                }
            }
        }
    }
    None
}

// ===========================================================================
// Rule 1 (path-level) + Rule 2 (content-level), over the live tree.
// ===========================================================================

#[test]
fn analysis_inputs_never_leak_into_tracked_artifacts() {
    let root = repo_root();
    let paths = tracked_paths(&root);
    // Unset ⇒ the token class is skipped (reason printed); the path-level and
    // path-shape rules below still run over the whole tree.
    let tokens = private_tokens_from_env().unwrap_or_default();
    assert!(
        paths.len() > 1000,
        "suspiciously few tracked paths ({}) — enumeration is broken",
        paths.len()
    );

    let allowed_example = allowed_example_rel();
    let banned_config = banned_live_config_rel();
    let analysis_prefix = format!("{}/", ".analysis");

    let mut violations: Vec<String> = Vec::new();

    // --- Rule 1: path-level ---
    let mut saw_example = false;
    for path in &paths {
        let norm = path.replace('\\', "/");
        if norm == banned_config {
            violations.push(format!("the live analysis config is tracked: {norm}"));
        }
        if norm == allowed_example {
            saw_example = true;
        } else if norm.starts_with(&analysis_prefix) {
            violations.push(format!(
                "unexpected tracked file under .analysis/ (only the example is allowed): {norm}"
            ));
        }
    }
    let _ = saw_example; // presence of the example is asserted by the D2 parse test.

    // --- Rule 2: content-level ---
    for path in &paths {
        let norm = path.replace('\\', "/");
        let abs = root.join(path);
        let bytes = match std::fs::read(&abs) {
            Ok(b) => b,
            Err(_) => continue, // a path that does not resolve on this checkout
        };
        if !is_text(&norm, &bytes) {
            continue;
        }
        let content = String::from_utf8_lossy(&bytes);
        if let Some(reason) = scan_text_artifact(&norm, &content, &tokens) {
            violations.push(format!("{norm}: {reason}"));
        }
    }

    assert!(
        violations.is_empty(),
        "local-analysis-input leak(s) found in {} tracked artifact(s):\n  {}",
        violations.len(),
        violations.join("\n  ")
    );
}

// ===========================================================================
// §1a anchor: the predicate DISCRIMINATES, independent of the live tree.
// ===========================================================================

#[test]
fn leak_predicate_flags_planted_leaks_and_passes_clean_artifacts() {
    // Synthetic stand-in tokens, fed through the SAME parser the environment value
    // goes through: two plain tokens (one upper-cased, one padded) and one
    // path-combo token written with backslashes.
    let tokens = parse_private_tokens("Acme-Widgets,\n  zetacorp  ,,VAULT\\acme");
    assert_eq!(tokens, ["acme-widgets", "zetacorp", "vault/acme"]);
    let org = "zetacorp";
    let ui = "acme-widgets";
    let drive = "d:";
    let dev = "dev";

    // ---- (1) CLASS 1: a planted private TOKEN in ANY text file is flagged ----
    // Direct token predicate over arbitrary text.
    assert!(token_leak(&format!("the {ui} library"), &tokens).is_some());
    assert!(token_leak(&format!("build broke in {org} repo"), &tokens).is_some());
    // The path-combo token matches case-insensitively across any separator run of
    // either slash kind.
    for combo in [
        "VAULT/acme/pkg",
        "Vault//Acme/pkg",
        "vault\\acme\\pkg",
        "vault\\\\acme",
    ] {
        assert!(
            token_leak(combo, &tokens).is_some(),
            "combo not flagged: {combo:?}"
        );
    }
    // The diagnostic names the token by index only — never by value.
    let reason = token_leak(&format!("x {org} y"), &tokens).expect("flagged");
    assert_eq!(reason, "private token #1");
    // A token rides into a plain `.rs`/`.md`/`.json` artifact — flagged regardless
    // of whether the artifact is shape-scanned (tokens are tree-wide).
    let token_in_source = format!("// note: regression from {ui}\nfn x() {{}}");
    assert!(scan_text_artifact("crates/foo/src/lib.rs", &token_in_source, &tokens).is_some());
    // A token inside a decoded JSON string value (no raw private slash).
    let token_json = format!(r#"{{"note":"build broke in {ui} pipeline"}}"#);
    assert!(
        scan_text_artifact("docs/whatever.json", &token_json, &tokens).is_some(),
        "private token in a JSON string value must be flagged tree-wide"
    );
    // A path-combo token hidden behind JSON-escaped backslashes is caught by the
    // decoded-string pass.
    let escaped_combo_json = r#"{"file":"vault\\acme\\src\\App.vue"}"#;
    assert!(scan_text_artifact("docs/whatever.json", escaped_combo_json, &tokens).is_some());
    // With NO configured token the same artifact is not token-flagged (the skipped
    // class is genuinely inert, not a hidden default list).
    assert_eq!(
        scan_text_artifact("crates/foo/src/lib.rs", &token_in_source, &[]),
        None
    );

    // ---- (2) CLASS 2: a generic shape in a CAMPAIGN ARTIFACT is flagged ----
    // Shape rules need no private data: they are exercised with an EMPTY token list.
    // A source map whose `sources` embeds a private absolute path (drive/dev shape).
    let leaky_sourcemap = format!(
        r#"{{"version":3,"sources":["/{drive}/{dev}/somewhere/widgets/src/App.vue"],"mappings":"AAAA"}}"#
    );
    assert!(
        scan_text_artifact(
            "packages/dx-harness/out/dx-events.map",
            &leaky_sourcemap,
            &[]
        )
        .is_some(),
        "source-map sources leak must be flagged in a shape-scanned artifact"
    );
    // A `/Users/<name>/...` shape (NO token) inside a campaign `.jsonl` event stream.
    let leaky_jsonl = r#"{"event":"open","file":"/Users/alice/project/src/main.ts"}"#;
    assert!(
        scan_text_artifact("packages/dx-harness/out/dx-events.jsonl", leaky_jsonl, &[]).is_some(),
        "a /Users/ shape in a campaign JSONL must be flagged"
    );
    // The ledger files are shape-scanned too.
    let leaky_ledger = format!(r#"{{"note":"see C:/{dev}/thing/file.ts"}}"#);
    assert!(
        scan_text_artifact(
            "scripts/manifests/replacement-deviations.json",
            &leaky_ledger,
            &[]
        )
        .is_some(),
        "a drive-root shape in the ledger must be flagged"
    );

    // ---- (3) SCOPE SPLIT: the SAME synthetic shape in a NON-artifact path/URI
    //          test fixture is NOT flagged (proves Class 2 is artifact-scoped) ----
    let synthetic_users = "/Users/alice/project/src/main.ts";
    let synthetic_home = "file:///home/bob/app/index.ts";
    let synthetic_drive = format!("C:/{dev}/thing/file.ts");
    for (path, body) in [
        // a path-canonicalization unit test (like verter_span/src/path.rs)
        ("crates/verter_span/src/path.rs", synthetic_users),
        // a URI test fixture (like verter_lsp/src/uri.rs)
        ("crates/verter_lsp/src/uri.rs", synthetic_home),
        // a TS path-normalization spec (like engine-key.spec.ts)
        (
            "packages/component-meta/src/runtime/engine-key.spec.ts",
            synthetic_drive.as_str(),
        ),
        // a doc with the dev's own machine path
        ("docs/some-report.md", synthetic_users),
    ] {
        let src = format!("const p = \"{body}\";");
        assert_eq!(
            scan_text_artifact(path, &src, &tokens),
            None,
            "synthetic shape in a non-artifact fixture must NOT be flagged: {path}"
        );
    }
    // But the very SAME synthetic body, when it appears in a shape-scanned artifact
    // path, IS flagged — the scope split, proven on identical content.
    let same_body_as_artifact = format!("const p = \"{synthetic_users}\";");
    assert!(
        scan_text_artifact("x.map", &same_body_as_artifact, &tokens).is_some(),
        "the same shape IS flagged in a shape-scanned artifact"
    );

    // ---- clean inputs neither class flags ----
    for clean in [
        "/path/to/project/src/App.vue",
        "/tmp/scratch/out.txt",
        "crates/verter_compiler/src/lib.rs",
        "analysis://p0001/file-0001.vue",
        "see ./README.md and ../docs/guide.md",
        "the developer ran the build", // 'dev' word, not a drive/posix-root shape
        "there are many users of this API", // 'users' substring, not a rooted path
        "node_modules/.bin/tsgo",
        "the vault and the acme team", // both combo halves, but not adjacent
    ] {
        assert_eq!(
            token_leak(clean, &tokens),
            None,
            "token false-positive: {clean:?}"
        );
        assert_eq!(
            path_shape_leak(clean),
            None,
            "shape false-positive: {clean:?}"
        );
    }
    // A clean source map (opaque virtual sources, null sourcesContent) passes.
    let clean_sourcemap = r#"{"version":3,"sources":["analysis://p0001/file-0001.vue"],"sourcesContent":[null],"mappings":"AAAA"}"#;
    assert_eq!(
        scan_text_artifact("clean.map", clean_sourcemap, &tokens),
        None
    );
}

#[test]
fn is_shape_scanned_artifact_discriminates() {
    // Campaign / generated-artifact surfaces are shape-scanned.
    assert!(is_shape_scanned_artifact(
        "packages/dx-harness/out/dx-events.map"
    ));
    assert!(is_shape_scanned_artifact(
        "packages/dx-harness/out/dx-events.jsonl"
    ));
    assert!(is_shape_scanned_artifact("some/snapshot.snap"));
    assert!(is_shape_scanned_artifact(
        "scripts/manifests/replacement-deviations.json"
    ));
    assert!(is_shape_scanned_artifact(
        "scripts/manifests/replacement-deviations.md"
    ));
    assert!(is_shape_scanned_artifact(&allowed_example_rel()));
    // CHANGELOG.md is machine-generated from commit subjects — a genuine path-leak
    // vector — so it IS shape-scanned (tightened: previously exempted).
    assert!(is_shape_scanned_artifact("CHANGELOG.md"));
    // General source / hand-authored docs / path-fixtures are NOT (they legitimately
    // carry the dev's own repo path + synthetic path examples).
    assert!(!is_shape_scanned_artifact("crates/verter_span/src/path.rs"));
    assert!(!is_shape_scanned_artifact("crates/verter_lsp/src/uri.rs"));
    assert!(!is_shape_scanned_artifact("docs/some-report.md"));
    assert!(!is_shape_scanned_artifact(
        "packages/component-meta/src/runtime/engine-key.spec.ts"
    ));
    // The off-limits verter_session perf goldens are NOT shape-scanned (out of
    // scope for this block; signed-off follow-up for the verter_session owner).
    assert!(!is_shape_scanned_artifact(
        "crates/verter_session/tests/perf_bounds/golden-corpus/representative-5.json"
    ));
    assert!(!is_shape_scanned_artifact(
        "crates/verter_session/tests/perf_bounds/golden-semantic/keys-eager.json"
    ));
}

/// The optional-separator class every redactor pattern must place between its
/// literal fragments.
const REDACTOR_SEPARATOR_CLASS: &str = "[-_ .]?";

/// Longest literal fragment a redactor pattern may spell between separator classes.
/// Short fragments keep a pattern from carrying a recognisable word of the name it
/// redacts.
const REDACTOR_MAX_FRAGMENT_LEN: usize = 4;

/// The `pattern = "..."` values of the redactor entries (`{ pattern = ..., replace = ... }`).
fn redactor_patterns(cliff: &str) -> Vec<String> {
    cliff
        .lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix("{ pattern = \"")?;
            let end = rest.find('"')?;
            Some(rest[..end].to_string())
        })
        .collect()
}

/// First redactor pattern (by index) spelling a literal fragment longer than
/// [`REDACTOR_MAX_FRAGMENT_LEN`], if any.
fn first_overlong_redactor_fragment(patterns: &[String]) -> Option<usize> {
    patterns.iter().position(|p| {
        p.replace("(?i)", "")
            .split(REDACTOR_SEPARATOR_CLASS)
            .any(|fragment| fragment.chars().count() > REDACTOR_MAX_FRAGMENT_LEN)
    })
}

/// What a redactor pattern matches when every optional separator is EMPTY.
fn redactor_pattern_no_separator_form(pattern: &str) -> String {
    pattern
        .replace("(?i)", "")
        .replace(REDACTOR_SEPARATOR_CLASS, "")
        .to_ascii_lowercase()
}

#[test]
fn changelog_redactor_is_structurally_fragmented_and_covers_configured_tokens() {
    let root = repo_root();
    let changelog = std::fs::read_to_string(root.join("CHANGELOG.md")).expect("read CHANGELOG.md");
    let cliff = std::fs::read_to_string(root.join("cliff.toml")).expect("read cliff.toml");

    // ---- Always-on: needs no private data ----
    // git-cliff regeneration must stay clean: cliff.toml declares a postprocess
    // redactor. Without it, the next `git-cliff` run could reintroduce a private
    // name from a commit subject. A future removal of the redactor is caught here.
    assert!(
        cliff.contains("postprocess"),
        "cliff.toml must declare a postprocess redactor so regeneration stays clean"
    );
    let patterns = redactor_patterns(&cliff);
    assert!(
        !patterns.is_empty(),
        "cliff.toml's postprocess redactor declares no pattern"
    );
    // Every pattern is case-insensitive and split into short fragments across the
    // separator class, so the config never spells a recognisable name and still
    // matches it with or without internal separators.
    for (idx, pattern) in patterns.iter().enumerate() {
        assert!(
            pattern.starts_with("(?i)") && pattern.contains(REDACTOR_SEPARATOR_CLASS),
            "cliff.toml redactor pattern #{idx} must be case-insensitive and fragmented \
             across the separator class {REDACTOR_SEPARATOR_CLASS}"
        );
    }
    assert_eq!(
        first_overlong_redactor_fragment(&patterns),
        None,
        "a cliff.toml redactor pattern spells a literal fragment longer than \
         {REDACTOR_MAX_FRAGMENT_LEN} chars; split it across the separator class"
    );
    // The fragment rule discriminates: an unfragmented or half-fragmented pattern
    // is rejected, a fully fragmented one passes.
    let sep = REDACTOR_SEPARATOR_CLASS;
    assert_eq!(
        first_overlong_redactor_fragment(&["(?i)acmewidgets".to_string()]),
        Some(0)
    );
    assert_eq!(
        first_overlong_redactor_fragment(&[
            format!("(?i)ac{sep}me{sep}wid{sep}gets"),
            format!("(?i)acme{sep}widgets"),
        ]),
        Some(1)
    );
    assert_eq!(
        first_overlong_redactor_fragment(&[format!("(?i)ac{sep}me{sep}wid{sep}gets")]),
        None
    );

    // ---- Private-token class: only when the token list is supplied ----
    let Some(tokens) = private_tokens_from_env() else {
        return;
    };
    assert_eq!(
        token_leak(&changelog, &tokens),
        None,
        "CHANGELOG.md must contain no private token"
    );
    assert_eq!(
        token_leak(&cliff, &tokens),
        None,
        "cliff.toml's redactor must not spell a private token contiguously"
    );
    // TARGETING — proven on the pattern's actual match set: with every optional
    // separator empty, some pattern must reconstruct to the no-separator form of
    // each configured name token (the form a commit subject would carry). Path-combo
    // tokens (containing `/`) are not commit-subject names and are not redactor
    // targets. Diagnostics report the token INDEX only.
    let reconstructed: Vec<String> = patterns
        .iter()
        .map(|p| redactor_pattern_no_separator_form(p))
        .collect();
    for (idx, token) in tokens.iter().enumerate() {
        if token.contains('/') {
            continue;
        }
        let no_sep: String = token
            .chars()
            .filter(|c| !matches!(c, '-' | '_' | ' ' | '.'))
            .collect();
        assert!(
            reconstructed.contains(&no_sep),
            "no cliff.toml redactor pattern reconstructs (with empty separators) to \
             configured private token #{idx} — the redactor would miss its no-separator \
             commit-subject form"
        );
    }
}

#[test]
fn gitignore_ignores_the_analysis_directory() {
    let root = repo_root();
    let gitignore = std::fs::read_to_string(root.join(".gitignore")).expect("read .gitignore");
    let dir_token = ".analysis";
    // An ignore line for the directory exists (either `.analysis/` or `.analysis/*`).
    let has_ignore = gitignore.lines().any(|l| {
        let t = l.trim();
        !t.starts_with('#')
            && !t.starts_with('!')
            && (t == format!("{dir_token}/") || t == format!("{dir_token}/*") || t == dir_token)
    });
    assert!(
        has_ignore,
        ".gitignore must ignore the {dir_token}/ directory"
    );
}

#[test]
fn path_level_rule_helpers_discriminate() {
    // The allowed example and the banned live config are distinct, and both live
    // under `.analysis/`.
    let example = allowed_example_rel();
    let banned = banned_live_config_rel();
    assert_ne!(example, banned);
    assert!(example.starts_with(".analysis/"));
    assert!(banned.starts_with(".analysis/"));
    // The example is the banned name plus a `.example` suffix.
    assert_eq!(example, format!("{banned}.example"));
}
